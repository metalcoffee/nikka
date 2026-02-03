#![deny(warnings)]
#![feature(allocator_api)]
#![feature(slice_ptr_get)]

use std::{
    alloc::{
        GlobalAlloc,
        Layout,
    },
    cmp,
    marker::Sync,
    mem,
    thread,
    time::{
        Duration,
        Instant,
    },
};

use derive_more::{
    Add,
    Display,
    Sum,
};
use rand::{
    Rng,
    SeedableRng,
    rngs::SmallRng,
};

use ku::{
    allocator::{
        Clip,
        DetailedInfo,
        Dispatcher,
        FIXED_SIZE_COUNT,
        Info,
    },
    log::{
        debug,
        error,
        info,
    },
    memory::{
        Size,
        Virt,
        size,
    },
    sync::Spinlock,
};

use allocator::{
    Big,
    CachingBig,
    Fallback,
    ThreadLocalCache,
};

#[cfg(feature = "benchmark")]
use {
    blink_alloc::GlobalBlinkAlloc,
    dlmalloc::GlobalDlmalloc,
    frusa::Frusa2M,
    good_memory_allocator::{
        DEFAULT_ALIGNMENT_SUB_BINS_AMOUNT,
        DEFAULT_SMALLBINS_AMOUNT,
        SpinLockedAllocator as Good,
    },
    jemallocator::Jemalloc,
    ku::{
        allocator::{
            Cache,
            GlobalCache,
        },
        memory::{
            GiB,
            MiB,
            Page,
        },
    },
    mimalloc::MiMalloc,
    ring_alloc::OneRingAlloc,
    rlsf::GlobalTlsf,
    spin::Mutex,
    std::{
        alloc::System,
        env,
        fs,
        ptr::{
            self,
            NonNull,
        },
    },
    talc::{
        ErrOnOom,
        Span,
        Talc,
    },
    tcmalloc_better::TCMalloc,
    wee_alloc::WeeAlloc,
};

mod allocator;
mod log;

#[test]
fn basic() {
    static ALLOCATOR: Dispatcher<ThreadLocalCache, Fallback> =
        Dispatcher::new(ThreadLocalCache::new(), Fallback::new());

    info!(
        common_metadata = %Size::bytes(mem::size_of_val(&ALLOCATOR)),
        per_thread_cache = %Size::bytes(mem::size_of::<Clip>() * FIXED_SIZE_COUNT),
        "allocator metadata size",
    );

    let mut stats = Stats::default();

    let layout = Layout::from_size_align(8, 8).unwrap();

    let ptr = unsafe { ALLOCATOR.alloc(layout) };
    assert!(!ptr.is_null());
    stats.allocation.count += 1;
    stats.allocations += 1;
    stats.requested += layout.size();

    unsafe {
        ALLOCATOR.dealloc(ptr, layout);
    }
    stats.deallocation.count += 1;

    ALLOCATOR.unmap();

    if cfg!(feature = "forbid-leaks") {
        assert_eq!(Big::total_mapped(), 0, "leaked some physical frames");
        assert_eq!(Big::total_reserved(), 0, "leaked some virtual pages");
    }

    let detailed_info: Spinlock<DetailedInfo> = Spinlock::new(DetailedInfo::new());
    ALLOCATOR.detailed_info(&mut detailed_info.lock());

    validate_info(stats, &detailed_info.lock());

    assert_eq!(CachingBig::total_memory(), 0, "do you deallocate?");
}

#[test]
fn single_threaded() {
    static ALLOCATOR: Dispatcher<ThreadLocalCache, Fallback> =
        Dispatcher::new(ThreadLocalCache::new(), Fallback::new());

    let nikka_allocator_stats = stress(
        "nikka allocator",
        &ALLOCATOR,
        SINGLE_THREADED_ITERATIONS,
        SEED,
    );

    ALLOCATOR.unmap();

    if cfg!(feature = "forbid-leaks") {
        assert_eq!(Big::total_mapped(), 0, "leaked some physical frames");
        assert_eq!(Big::total_reserved(), 0, "leaked some virtual pages");
    }

    let detailed_info: Spinlock<DetailedInfo> = Spinlock::new(DetailedInfo::new());
    ALLOCATOR.detailed_info(&mut detailed_info.lock());

    validate_info(nikka_allocator_stats, &detailed_info.lock());

    ALLOCATOR.detailed_info(&mut detailed_info.lock());
    validate_info(nikka_allocator_stats, &detailed_info.lock());
    validate_info_empty(&detailed_info.lock());

    let nikka_pure_operations_ns =
        nikka_allocator_stats.pure_operations().nanoseconds_per_operation();
    info!(
        "nikka allocation + deallocation duration = {:0.2?}ns",
        nikka_pure_operations_ns,
    );

    #[cfg(feature = "benchmark")]
    {
        let tcmalloc_stats = stress("tcmalloc", &TCMalloc, SINGLE_THREADED_ITERATIONS, SEED);
        let jemalloc_stats = stress("jemalloc", &Jemalloc, SINGLE_THREADED_ITERATIONS, SEED);
        let mimalloc_stats = stress("mimalloc", &MiMalloc, SINGLE_THREADED_ITERATIONS, SEED);

        let ring = Ring(OneRingAlloc);
        let ring_stats = stress("ring-alloc", &ring, SINGLE_THREADED_ITERATIONS, SEED);

        static ALLOCATOR_GLOBAL_CACHE: Dispatcher<GlobalCache, Fallback> =
            Dispatcher::new(GlobalCache::new(), Fallback::new());
        let nikka_allocator_global_cache_stats = stress(
            "nikka allocator (global cache)",
            &ALLOCATOR_GLOBAL_CACHE,
            SINGLE_THREADED_ITERATIONS,
            SEED,
        );
        ALLOCATOR_GLOBAL_CACHE.unmap();

        static ALLOCATOR_NO_CACHE: Dispatcher<NoCache, Fallback> =
            Dispatcher::new(NoCache, Fallback::new());
        let nikka_allocator_no_cache_stats = stress(
            "nikka allocator (no cache)",
            &ALLOCATOR_NO_CACHE,
            SINGLE_THREADED_ITERATIONS,
            SEED,
        );
        ALLOCATOR_NO_CACHE.unmap();

        let system_allocator_stats = stress(
            "system allocator",
            &System,
            SINGLE_THREADED_ITERATIONS,
            SEED,
        );

        let dlmalloc_stats = stress(
            "dlmalloc",
            &GlobalDlmalloc,
            SINGLE_THREADED_ITERATIONS,
            SEED,
        );

        let blink = GlobalBlinkAlloc::new();
        let blink_stats = stress("blink", &blink, SINGLE_THREADED_ITERATIONS, SEED);

        let frusa = Frusa2M::new(&ALLOCATOR);
        let frusa_stats = stress("frusa", &frusa, SINGLE_THREADED_ITERATIONS, SEED);
        ALLOCATOR.unmap();

        let rlsf = GlobalTlsf::<()>::new();
        let rlsf_stats = stress("rlsf", &rlsf, SINGLE_THREADED_ITERATIONS, SEED);

        let mut talc = Talc::new(ErrOnOom);
        let talc_stats = unsafe {
            let size = MAX_TOTAL_MEMORY_PER_ALLOCATOR;
            let layout = Page::layout(size).unwrap();
            let start = ALLOCATOR.alloc(layout);
            assert!(!start.is_null());
            let end = start.add(size);
            talc.claim(Span::new(start, end)).unwrap();
            let talck = talc.lock::<Mutex<()>>();
            let talc_stats = stress("talc", &talck, SINGLE_THREADED_ITERATIONS, SEED);
            ALLOCATOR.dealloc(start, layout);
            ALLOCATOR.unmap();
            talc_stats
        };

        let wee = WeeAlloc::INIT;
        let wee_stats = stress("wee", &wee, SINGLE_THREADED_ITERATIONS, SEED);

        let good = Good::<DEFAULT_SMALLBINS_AMOUNT, DEFAULT_ALIGNMENT_SUB_BINS_AMOUNT>::empty();
        let good_stats = unsafe {
            let size = MAX_TOTAL_MEMORY_PER_ALLOCATOR;
            let layout = Page::layout(size).unwrap();
            let start = ALLOCATOR.alloc(layout);
            assert!(!start.is_null());
            good.init(start as usize, size);
            let good_stats = stress("good", &good, SINGLE_THREADED_ITERATIONS, SEED);
            ALLOCATOR.dealloc(start, layout);
            ALLOCATOR.unmap();
            good_stats
        };

        validate_performance(
            nikka_allocator_stats,
            tcmalloc_stats,
            jemalloc_stats,
            mimalloc_stats,
            system_allocator_stats,
            None,
            1,
        );

        if env::var("UPDATE_CHARTS").is_ok() {
            chart_duration(
                "Single threaded allocator performance",
                &[
                    ("nikka allocator", &nikka_allocator_stats),
                    ("tcmalloc", &tcmalloc_stats),
                    ("jemalloc", &jemalloc_stats),
                    ("mimalloc", &mimalloc_stats),
                    ("ring-alloc", &ring_stats),
                    (
                        "nikka allocator (global cache)",
                        &nikka_allocator_global_cache_stats,
                    ),
                    (
                        "nikka allocator (no cache)",
                        &nikka_allocator_no_cache_stats,
                    ),
                    ("system", &system_allocator_stats),
                    ("dlmalloc", &dlmalloc_stats),
                    ("blink", &blink_stats),
                    ("frusa", &frusa_stats),
                    ("rlsf", &rlsf_stats),
                    ("talc", &talc_stats),
                    ("wee", &wee_stats),
                    ("good", &good_stats),
                ],
                "../lab/src/3-allocator-6-singlethreaded-performance.tex",
            );

            chart_duration(
                "Single threaded allocator performance",
                &[
                    ("nikka allocator", &nikka_allocator_stats),
                    ("tcmalloc", &tcmalloc_stats),
                    ("jemalloc", &jemalloc_stats),
                    ("mimalloc", &mimalloc_stats),
                    ("ring-alloc", &ring_stats),
                ],
                "../lab/src/3-allocator-6-singlethreaded-performance-no-system.tex",
            );

            chart_ops_per_us(
                "Single threaded allocator performance",
                &[
                    ("nikka allocator", &nikka_allocator_stats),
                    ("tcmalloc", &tcmalloc_stats),
                    ("jemalloc", &jemalloc_stats),
                ],
                "../lab/src/3-allocator-6-singlethreaded-performance-ops-per-us.tex",
            );
        }
    }
}

#[test]
fn multi_threaded() {
    static ALLOCATOR: Dispatcher<ThreadLocalCache, Fallback> =
        Dispatcher::new(ThreadLocalCache::new(), Fallback::new());

    let thread_count = thread::available_parallelism().unwrap().get();

    let nikka_allocator_stats = multi_threaded_stress("nikka allocator", &ALLOCATOR, thread_count);

    ALLOCATOR.unmap();

    if cfg!(feature = "forbid-leaks") {
        assert_eq!(Big::total_mapped(), 0, "leaked some physical frames");
        assert_eq!(Big::total_reserved(), 0, "leaked some virtual pages");
    }

    let detailed_info: Spinlock<DetailedInfo> = Spinlock::new(DetailedInfo::new());
    ALLOCATOR.detailed_info(&mut detailed_info.lock());

    validate_info(nikka_allocator_stats, &detailed_info.lock());

    ALLOCATOR.detailed_info(&mut detailed_info.lock());
    validate_info(nikka_allocator_stats, &detailed_info.lock());
    validate_info_empty(&detailed_info.lock());

    let nikka_pure_operations_ns =
        nikka_allocator_stats.pure_operations().nanoseconds_per_operation();
    info!(
        "nikka allocation + deallocation duration = {:0.2?}ns",
        nikka_pure_operations_ns,
    );

    #[cfg(feature = "benchmark")]
    {
        let tcmalloc_stats = multi_threaded_stress("tcmalloc", &TCMalloc, thread_count);
        let jemalloc_stats = multi_threaded_stress("jemalloc", &Jemalloc, thread_count);
        let mimalloc_stats = multi_threaded_stress("mimalloc", &MiMalloc, thread_count);

        let ring = Ring(OneRingAlloc);
        let ring_stats = multi_threaded_stress("ring-alloc", &ring, thread_count);

        static ALLOCATOR_GLOBAL_CACHE: Dispatcher<GlobalCache, Fallback> =
            Dispatcher::new(GlobalCache::new(), Fallback::new());
        let nikka_allocator_global_cache_stats = multi_threaded_stress(
            "nikka allocator (global cache)",
            &ALLOCATOR_GLOBAL_CACHE,
            thread_count,
        );
        ALLOCATOR_GLOBAL_CACHE.unmap();

        static ALLOCATOR_NO_CACHE: Dispatcher<NoCache, Fallback> =
            Dispatcher::new(NoCache, Fallback::new());
        let nikka_allocator_no_cache_stats = multi_threaded_stress(
            "nikka allocator (no cache)",
            &ALLOCATOR_NO_CACHE,
            thread_count,
        );
        ALLOCATOR_NO_CACHE.unmap();

        let system_allocator_stats =
            multi_threaded_stress("system allocator", &System, thread_count);

        let dlmalloc_stats = multi_threaded_stress("dlmalloc", &GlobalDlmalloc, thread_count);

        let blink = GlobalBlinkAlloc::new();
        let blink_stats = multi_threaded_stress("blink", &blink, thread_count);

        static FRUSA: Frusa2M = Frusa2M::new(&ALLOCATOR);
        let frusa_stats = multi_threaded_stress("frusa", &FRUSA, thread_count);
        ALLOCATOR.unmap();

        let rlsf = GlobalTlsf::<()>::new();
        let rlsf_stats = multi_threaded_stress("rlsf", &rlsf, thread_count);

        let mut talc = Talc::new(ErrOnOom);
        let talc_stats = unsafe {
            let size = MAX_TOTAL_MEMORY_PER_ALLOCATOR;
            let layout = Page::layout(size).unwrap();
            let start = ALLOCATOR.alloc(layout);
            assert!(!start.is_null());
            let end = start.add(size);
            talc.claim(Span::new(start, end)).unwrap();
            let talck = talc.lock::<Mutex<()>>();
            let talc_stats = multi_threaded_stress("talc", &talck, thread_count);
            ALLOCATOR.dealloc(start, layout);
            ALLOCATOR.unmap();
            talc_stats
        };

        let wee = WeeAlloc::INIT;
        let wee_stats = multi_threaded_stress("wee", &wee, thread_count);

        validate_performance(
            nikka_allocator_stats,
            tcmalloc_stats,
            jemalloc_stats,
            mimalloc_stats,
            system_allocator_stats,
            Some(1.1),
            thread_count,
        );

        if env::var("UPDATE_CHARTS").is_ok() {
            chart_duration(
                "Multithreaded allocator performance",
                &[
                    ("nikka allocator", &nikka_allocator_stats),
                    ("tcmalloc", &tcmalloc_stats),
                    ("jemalloc", &jemalloc_stats),
                    ("mimalloc", &mimalloc_stats),
                    ("ring-alloc", &ring_stats),
                    (
                        "nikka allocator (global cache)",
                        &nikka_allocator_global_cache_stats,
                    ),
                    (
                        "nikka allocator (no cache)",
                        &nikka_allocator_no_cache_stats,
                    ),
                    ("system", &system_allocator_stats),
                    ("dlmalloc", &dlmalloc_stats),
                    ("blink", &blink_stats),
                    ("frusa", &frusa_stats),
                    ("rlsf", &rlsf_stats),
                    ("talc", &talc_stats),
                    ("wee", &wee_stats),
                ],
                "../lab/src/3-allocator-6-multithreaded-performance.tex",
            );

            chart_duration(
                "Multithreaded allocator performance",
                &[
                    ("nikka allocator", &nikka_allocator_stats),
                    ("tcmalloc", &tcmalloc_stats),
                    ("jemalloc", &jemalloc_stats),
                    ("mimalloc", &mimalloc_stats),
                    ("ring-alloc", &ring_stats),
                ],
                "../lab/src/3-allocator-6-multithreaded-performance-no-system.tex",
            );

            chart_ops_per_us(
                "Multithreaded allocator performance",
                &[
                    ("nikka allocator", &nikka_allocator_stats),
                    ("tcmalloc", &tcmalloc_stats),
                    ("jemalloc", &jemalloc_stats),
                ],
                "../lab/src/3-allocator-6-multithreaded-performance-ops-per-us.tex",
            );
        }
    }

    fn multi_threaded_stress(
        allocator_name: &str,
        allocator: &(impl GlobalAlloc + Sync),
        thread_count: usize,
    ) -> Stats {
        thread::scope(|scope| {
            let threads: Vec<_> = (0 .. thread_count)
                .map(|thread| {
                    scope.spawn(move || {
                        stress(
                            allocator_name,
                            allocator,
                            MULTI_THREADED_ITERATIONS,
                            SEED + thread,
                        )
                    })
                })
                .collect();

            threads
                .into_iter()
                .map(|thread| thread.join().expect("threads should finish successfully"))
                .sum()
        })
    }
}

fn validate_info(
    stats: Stats,
    detailed_info: &DetailedInfo,
) {
    assert_eq!(stats.allocation.count, stats.deallocation.count);

    if !cfg!(feature = "allocator-statistics") {
        return;
    }

    info!(total = %detailed_info.total());
    println!("{detailed_info:#}");

    assert!(detailed_info.is_valid());

    let total = detailed_info.total();
    assert_eq!(total.allocations().positive(), stats.allocations);
    assert!(total.allocated().positive() >= stats.requested);
    assert_eq!(total.requested().positive(), stats.requested);
    assert_eq!(total.allocations().balance(), 0);
    assert_eq!(total.allocated().balance(), 0);
    assert_eq!(total.requested().balance(), 0);
    assert_eq!(total.allocated().balance(), 0);
    assert!(total.pages().positive() > 0);
}

fn validate_info_empty(detailed_info: &DetailedInfo) {
    validate_empty(detailed_info.total());
    validate_empty(detailed_info.fallback());

    for info in detailed_info.fixed_size().iter() {
        validate_empty(info);
    }

    fn validate_empty(info: &Info) {
        assert_eq!(info.allocations().balance(), 0);
        assert_eq!(info.allocated().balance(), 0);
        assert_eq!(info.requested().balance(), 0);
        assert_eq!(info.pages().balance(), 0);
    }
}

#[cfg(feature = "benchmark")]
fn validate_performance(
    nikka_allocator_stats: Stats,
    tcmalloc_stats: Stats,
    jemalloc_stats: Stats,
    mimalloc_stats: Stats,
    system_allocator_stats: Stats,
    allowed_ratio: Option<f32>,
    thread_count: usize,
) {
    let nikka = nikka_allocator_stats.total().duration;
    let tcmalloc = tcmalloc_stats.total().duration;
    let jemalloc = jemalloc_stats.total().duration;
    let mimalloc = mimalloc_stats.total().duration;
    let system = system_allocator_stats.total().duration;

    let nikka_div_tcmalloc = nikka.div_duration_f32(tcmalloc);
    let nikka_div_jemalloc = nikka.div_duration_f32(jemalloc);
    let nikka_div_mimalloc = nikka.div_duration_f32(mimalloc);
    let nikka_div_system = nikka.div_duration_f32(system);
    info!(
        ?nikka,
        ?tcmalloc,
        ?jemalloc,
        ?mimalloc,
        ?system,
        nikka_div_tcmalloc,
        nikka_div_jemalloc,
        nikka_div_mimalloc,
        nikka_div_system,
        "performance (smaller is better)",
    );

    let allowed_ratio = env::var("ALLOWED_RATIO")
        .ok()
        .map(|x| x.parse::<f32>().unwrap())
        .or(allowed_ratio);

    compare(
        "tcmalloc",
        &tcmalloc_stats,
        &nikka_allocator_stats,
        allowed_ratio,
    );
    compare(
        "jemalloc",
        &jemalloc_stats,
        &nikka_allocator_stats,
        allowed_ratio,
    );
    compare(
        "mimalloc",
        &mimalloc_stats,
        &nikka_allocator_stats,
        allowed_ratio,
    );
    compare(
        "system",
        &system_allocator_stats,
        &nikka_allocator_stats,
        None,
    );

    let total_memory = CachingBig::total_memory();
    info!(total_memory = %Size::bytes(total_memory));
    let frusa_leaks = 10 * MiB * thread_count;
    assert!(total_memory <= frusa_leaks, "do you deallocate?");

    fn compare(
        base_name: &str,
        base: &Stats,
        nikka: &Stats,
        allowed_ratio: Option<f32>,
    ) {
        let allocation = nikka.allocation.duration.div_duration_f32(base.allocation.duration);
        let allocation_zeroed = nikka
            .allocation_zeroed
            .duration
            .div_duration_f32(base.allocation_zeroed.duration);
        let deallocation = nikka.deallocation.duration.div_duration_f32(base.deallocation.duration);
        let reallocation = nikka.reallocation.duration.div_duration_f32(base.reallocation.duration);
        let pure_operations = nikka
            .pure_operations()
            .duration
            .div_duration_f32(base.pure_operations().duration);
        let mem_operations =
            nikka.mem_operations().duration.div_duration_f32(base.mem_operations().duration);
        let total = nikka.total().duration.div_duration_f32(base.total().duration);

        info!(
            allocation,
            allocation_zeroed,
            deallocation,
            reallocation,
            pure_operations,
            mem_operations,
            total,
            nikka_pure_operations_ns = nikka.pure_operations().nanoseconds_per_operation(),
            "performance breakdown of nikka allocator versus {base_name} (smaller is better)",
        );

        if let Some(allowed_ratio) = allowed_ratio {
            assert!(
                pure_operations <= allowed_ratio,
                "you can do better than {pure_operations} (limit is {allowed_ratio})",
            );
            assert!(
                mem_operations <= allowed_ratio,
                "you can do better than {mem_operations} (limit is {allowed_ratio})",
            );
            assert!(
                total <= allowed_ratio,
                "you can do better than {total} (limit is {allowed_ratio})",
            );
        }
    }
}

fn stress(
    allocator_name: &str,
    allocator: &impl GlobalAlloc,
    iterations: usize,
    seed: usize,
) -> Stats {
    let mut active: Vec<Allocation<_>> = Vec::new();
    let mut last_report = 0;
    let mut rng = SmallRng::seed_from_u64(size::into_u64(seed));
    let mut stats = Stats::default();
    let start = Instant::now();

    for iteration in 1 ..= iterations {
        let operation = rng.gen_range(0 .. 3);
        let index = if !active.is_empty() {
            rng.gen_range(0 .. active.len())
        } else {
            0
        };

        if last_report + (iterations / 10) <= iteration &&
            index < active.len() &&
            active[index].sum.is_some()
        {
            debug!(allocator = allocator_name, iteration, allocation = %active[index]);
            last_report = iteration;
        }

        if active.is_empty() || operation == 0 {
            let (allocation, duration) = Allocation::generate(allocator, &mut rng);
            if allocation.zeroed {
                stats.allocation_zeroed.register(duration);
            } else {
                stats.allocation.register(duration);
            }
            stats.allocations += 1;
            stats.requested += allocation.layout.size();
            active.push(allocation);
        } else if active.len() > MAX_ACTIVE_ALLOCATIONS || operation == 1 {
            free_allocation(&mut active, index, &mut stats);
        } else if operation == 2 {
            let duration = active[index].resize(&mut rng);
            stats.reallocation.register(duration);
            stats.allocations += 1;
            stats.requested += active[index].layout.size();
        }
    }

    while !active.is_empty() {
        free_allocation(&mut active, 0, &mut stats);
    }

    let end = Instant::now();
    stats.duration = end - start;

    if cfg!(feature = "benchmark") {
        info!(
            allocator_name,
            allocation = %stats.allocation,
            allocation_zeroed = %stats.allocation_zeroed,
            deallocation = %stats.deallocation,
            reallocation = %stats.reallocation,
            supplementary_code = ?stats.supplementary_duration(),
            total = ?stats.duration,
            "performance breakdown (smaller is better)",
        );
    }

    return stats;

    fn free_allocation<T: GlobalAlloc>(
        active: &mut Vec<Allocation<'_, T>>,
        index: usize,
        stats: &mut Stats,
    ) {
        let last = active.pop().unwrap();
        let to_be_freed = if index < active.len() {
            mem::replace(&mut active[index], last)
        } else {
            last
        };
        to_be_freed.check_sum();
        let register_deallocation = !to_be_freed.zeroed;
        let start = Instant::now();
        drop(to_be_freed);
        let duration = Instant::now() - start;
        if register_deallocation {
            stats.deallocation.register(duration);
        }
    }
}

#[derive(Display)]
#[display(
    "{{ size: {}, align: {}, ptr: {}, sum: {:?} }}",
    layout.size(),
    layout.align(),
    ptr,
    sum,
)]
struct Allocation<'a, T: GlobalAlloc> {
    allocator: &'a T,
    layout: Layout,
    ptr: Virt,
    sum: Option<usize>,
    zeroed: bool,
}

impl<'a, T: GlobalAlloc> Allocation<'a, T> {
    fn generate(
        allocator: &'a T,
        rng: &mut SmallRng,
    ) -> (Self, Duration) {
        let align = Self::generate_align(rng);
        let size = Self::generate_size(rng);
        let layout = Layout::from_size_align(size, align).unwrap();

        let zeroed = rng.gen_ratio(1, 2);

        let start = Instant::now();
        let ptr = if zeroed {
            unsafe { allocator.alloc_zeroed(layout) }
        } else {
            unsafe { allocator.alloc(layout) }
        };
        let duration = Instant::now() - start;
        let ptr = Virt::from_ptr(ptr);
        Self::check_ptr(layout, ptr);

        let check = if cfg!(feature = "benchmark") {
            rng.gen_ratio(1, BENCHMARK_CHECK_RATE)
        } else {
            true
        };
        let data = unsafe { ptr.try_into_mut_slice(layout.size()).unwrap() };
        if zeroed {
            assert_eq!(data.iter().find(|&&x| x != 0), None);
        }
        if check {
            rng.fill(data);
        }
        let sum = if check {
            Some(Self::sum(data))
        } else {
            None
        };

        (
            Self {
                allocator,
                layout,
                ptr,
                sum,
                zeroed,
            },
            duration,
        )
    }

    fn resize(
        &mut self,
        rng: &mut SmallRng,
    ) -> Duration {
        self.check_sum();

        let new_size = Self::generate_size(rng);
        let new_layout = Layout::from_size_align(new_size, self.layout.align()).unwrap();
        let common_size = cmp::min(self.layout.size(), new_size);

        let old_data = unsafe { self.ptr.try_into_slice(common_size).unwrap() };
        let old_sum = Self::sum(old_data);

        let start = Instant::now();
        let new_ptr =
            Virt::from_ptr(unsafe { self.allocator.realloc(self.ptr(), self.layout, new_size) });
        let duration = Instant::now() - start;
        Self::check_ptr(new_layout, new_ptr);

        let new_data = unsafe { new_ptr.try_into_slice(common_size).unwrap() };
        let new_sum = Self::sum(new_data);
        assert_eq!(old_sum, new_sum);

        let check = if cfg!(feature = "benchmark") {
            rng.gen_ratio(1, BENCHMARK_CHECK_RATE)
        } else {
            true
        };
        let data = unsafe { new_ptr.try_into_mut_slice(new_size).unwrap() };
        if check {
            rng.fill(data);
        }

        self.layout = new_layout;
        self.ptr = new_ptr;
        self.sum = if check {
            Some(Self::sum(data))
        } else {
            None
        };

        duration
    }

    fn check_ptr(
        layout: Layout,
        ptr: Virt,
    ) {
        let align_difference = ptr.into_usize() % layout.align();
        if align_difference != 0 {
            error!(?layout, %ptr, align_difference, "wrong alignment of allocated pointer");
        }
        assert_eq!(align_difference, 0);
    }

    fn check_sum(&self) {
        if let Some(sum) = self.sum {
            let data = unsafe { self.ptr.try_into_mut_slice(self.layout.size()).unwrap() };
            assert_eq!(Self::sum(data), sum);
        }
    }

    fn generate_align(rng: &mut SmallRng) -> usize {
        1 << rng.gen_range(0 ..= MAX_LB_ALIGN)
    }

    fn generate_size(rng: &mut SmallRng) -> usize {
        SIZE_MULTIPLIER * rng.gen_range(1 ..= DIFFERENT_SIZES)
    }

    fn ptr(&self) -> *mut u8 {
        self.ptr.try_into_mut_ptr().unwrap()
    }

    fn sum(data: &[u8]) -> usize {
        data.iter().map(|&x| usize::from(x)).sum()
    }
}

impl<T: GlobalAlloc> Drop for Allocation<'_, T> {
    fn drop(&mut self) {
        unsafe {
            self.allocator.dealloc(self.ptr(), self.layout);
        }
    }
}

#[derive(Add, Clone, Copy, Default, Sum)]
struct Stats {
    allocations: usize,
    requested: usize,
    duration: Duration,
    allocation: OperationStats,
    allocation_zeroed: OperationStats,
    deallocation: OperationStats,
    reallocation: OperationStats,
}

impl Stats {
    fn supplementary_duration(&self) -> Duration {
        self.duration -
            self.allocation.duration -
            self.allocation_zeroed.duration -
            self.deallocation.duration -
            self.reallocation.duration
    }

    fn pure_operations(&self) -> OperationStats {
        assert_eq!(self.allocation.count, self.deallocation.count);
        let mut pure_operations = self.allocation + self.deallocation;
        pure_operations.count = self.allocation.count;

        pure_operations
    }

    #[cfg(feature = "benchmark")]
    fn mem_operations(&self) -> OperationStats {
        self.allocation_zeroed + self.reallocation
    }

    #[cfg(feature = "benchmark")]
    fn total(&self) -> OperationStats {
        self.pure_operations() + self.mem_operations()
    }
}

#[derive(Add, Clone, Copy, Default, Display, Sum)]
#[display(
    "{{ total: {:?}, per_operation: {:0.2?}ns, count: {} }}",
    duration,
    self.nanoseconds_per_operation(),
    count,
)]
struct OperationStats {
    count: usize,
    ops_per_us: usize,
    ops_squared_per_us: usize,
    ms: usize,
    duration: Duration,
}

impl OperationStats {
    fn register(
        &mut self,
        duration: Duration,
    ) {
        self.count += 1;

        let start = Instant::now();
        let end = Instant::now();
        let noop_duration = end - start;

        if duration > noop_duration {
            let operation_duration = duration - noop_duration;
            let old_duration = self.duration;
            self.duration += operation_duration;
            if old_duration.as_micros() < self.duration.as_micros() {
                let ops_per_us = self.count - self.ops_per_us;
                self.ops_per_us += ops_per_us;
                self.ops_squared_per_us += ops_per_us * ops_per_us;
                self.ms += 1;
            }
        }
    }

    fn nanoseconds_per_operation(&self) -> f64 {
        u64::try_from(self.duration.as_nanos()).unwrap() as f64 /
            u64::try_from(self.count).unwrap() as f64
    }

    #[cfg(feature = "benchmark")]
    fn ops_per_us_mean(&self) -> f64 {
        if self.ms == 0 {
            0.0
        } else {
            let ms = into_f64(self.ms);
            let ops_per_us = into_f64(self.ops_per_us);

            ops_per_us / ms
        }
    }

    #[cfg(feature = "benchmark")]
    fn ops_per_us_standard_deviation(&self) -> f64 {
        if self.ms == 0 {
            0.0
        } else {
            let ms = into_f64(self.ms);
            let mean = self.ops_per_us_mean();
            let ops_squared_per_us = self.ops_squared_per_us as f64;

            (ops_squared_per_us / ms - mean * mean).sqrt()
        }
    }
}

#[cfg(feature = "benchmark")]
fn into_f64(x: usize) -> f64 {
    f64::from(u32::try_from(x).unwrap())
}

#[cfg(feature = "benchmark")]
struct NoCache;

#[cfg(feature = "benchmark")]
impl Cache for NoCache {
    fn with_borrow_mut<F: FnOnce(&mut Clip) -> R, R>(
        &self,
        _index: usize,
        _f: F,
    ) -> R {
        panic!("allocator cache is not available");
    }

    const CACHE_AVAILABLE: bool = false;
}

#[cfg(feature = "benchmark")]
fn chart_duration(
    title: &str,
    data: &[(&str, &Stats)],
    file: &str,
) {
    let mut chart = HEADER.to_owned();

    chart += &format!("    title={{{title}}},\n    width={}mm,\n", 35 * data.len());
    chart += "    ylabel={Operation duration, ns (smaller is better)},\n";
    chart += "    symbolic x coords=";
    chart += "{allocation,deallocation,allocation + deallocation, ,";
    chart += "zeroed allocation,reallocation},\n]";

    for (color, (_, stats)) in data.iter().enumerate() {
        chart += &format!(
            r##"
    \addplot [draw=black!35!color{color}, fill=black!10!color{color}] coordinates {{
        (allocation,{})
        (deallocation,{})
        (allocation + deallocation,{})
        (zeroed allocation,{})
        (reallocation,{})
    }};
"##,
            stats.allocation.nanoseconds_per_operation(),
            stats.deallocation.nanoseconds_per_operation(),
            stats.pure_operations().nanoseconds_per_operation(),
            stats.allocation_zeroed.nanoseconds_per_operation(),
            stats.reallocation.nanoseconds_per_operation(),
        );
    }

    chart += "\n    \\legend{";
    let mut separator = "";
    for (allocator_name, _) in data {
        chart += &format!("{separator}{allocator_name}");
        separator = ",";
    }
    chart += "}";

    chart += TAILER;

    if let Err(error) = fs::write(file, chart) {
        error!(%error, file, "failed to write the file");
    }
}

#[cfg(feature = "benchmark")]
fn chart_ops_per_us(
    title: &str,
    data: &[(&str, &Stats)],
    file: &str,
) {
    let mut chart = HEADER.to_owned();

    chart += &format!("    title={{{title}}},\n    width={}mm,\n", 40 * data.len());
    chart += "    ylabel={Operations per \\SI{}{\\micro\\second} (greater is better), ";
    chart += "errors show standard deviation},\n";
    chart += "    symbolic x coords=";
    chart += "{allocation,deallocation},\n]";

    for (color, (_, stats)) in data.iter().enumerate() {
        chart += &format!(
            r##"
    \addplot [
        draw=black!35!color{color},
        fill=black!10!color{color},
        error bars/.cd, y dir=both, y explicit,
    ] coordinates {{
        (allocation,{}) +- ({},{})
        (deallocation,{}) +- ({},{})
    }};
"##,
            stats.allocation.ops_per_us_mean(),
            stats.allocation.ops_per_us_standard_deviation(),
            stats.allocation.ops_per_us_standard_deviation(),
            stats.deallocation.ops_per_us_mean(),
            stats.deallocation.ops_per_us_standard_deviation(),
            stats.deallocation.ops_per_us_standard_deviation(),
        );
    }

    chart += "\n    \\legend{";
    let mut separator = "";
    for (allocator_name, _) in data {
        chart += &format!("{separator}{allocator_name}");
        separator = ",";
    }
    chart += "}";

    chart += TAILER;

    if let Err(error) = fs::write(file, chart) {
        error!(%error, file, "failed to write the file");
    }
}

#[cfg(feature = "benchmark")]
const HEADER: &str = r##"
\documentclass[border=2mm]{standalone}

\usepackage[dvipsnames]{xcolor}
\usepackage{pgfplots}
\usepackage{tikz}
\usepackage{siunitx}

\usetikzlibrary{arrows.meta}
\usetikzlibrary{decorations.pathreplacing,calligraphy}
\usetikzlibrary{math}
\usetikzlibrary{matrix}
\usetikzlibrary{positioning}
\usetikzlibrary{shapes.misc}

\begin{document}

\definecolor{color0}{RGB}{202, 218, 246}
\definecolor{color1}{RGB}{221, 253, 234}
\definecolor{color2}{RGB}{252, 244, 221}
\definecolor{color3}{RGB}{252, 225, 228}
\definecolor{color4}{RGB}{244, 202, 252}
\definecolor{color5}{RGB}{202, 252, 202}
\definecolor{color6}{RGB}{202, 202, 202}
\definecolor{color7}{RGB}{151, 151, 202}
\definecolor{color8}{RGB}{102, 118, 146}
\definecolor{color9}{RGB}{121, 153, 134}
\definecolor{color10}{RGB}{152, 144, 121}
\definecolor{color11}{RGB}{152, 125, 128}
\definecolor{color12}{RGB}{144, 102, 152}
\definecolor{color13}{RGB}{102, 152, 102}
\definecolor{color14}{RGB}{102, 102, 102}

\begin{tikzpicture}
\begin{axis}[
    ybar,
    height=150mm,
    legend cell align=left,
    legend pos=north west,
    nodes near coords, nodes near coords style={rotate=90,anchor=west},
    x tick label style={rotate=30,anchor=east},
    xtick=data,
    ymin=0,
"##;

#[cfg(feature = "benchmark")]
const TAILER: &str = r##"
\end{axis}
\end{tikzpicture}

\end{document}
"##;

#[cfg(feature = "benchmark")]
struct Ring(OneRingAlloc);

#[cfg(feature = "benchmark")]
unsafe impl GlobalAlloc for Ring {
    unsafe fn alloc(
        &self,
        layout: Layout,
    ) -> *mut u8 {
        self.0
            .allocate(layout)
            .map(|x| x.as_non_null_ptr().as_ptr())
            .unwrap_or_else(|_| ptr::null_mut())
    }

    unsafe fn dealloc(
        &self,
        ptr: *mut u8,
        layout: Layout,
    ) {
        unsafe {
            self.0.deallocate(NonNull::new(ptr).unwrap(), layout);
        }
    }
}

#[ctor::ctor]
fn init() {
    log::init();
}

const BENCHMARK_CHECK_RATE: u32 = 1_000;
const DIFFERENT_SIZES: usize = 1024;
const MAX_ACTIVE_ALLOCATIONS: usize = 1_000_000;
const MAX_LB_ALIGN: u32 = 10;
const MULTI_THREADED_ITERATIONS: usize = 1_000_000;
const SEED: usize = 314159265;
const SINGLE_THREADED_ITERATIONS: usize = 1_000_000;
const SIZE_MULTIPLIER: usize = 1;

#[cfg(feature = "benchmark")]
const MAX_TOTAL_MEMORY_PER_ALLOCATOR: usize = GiB / 2;
