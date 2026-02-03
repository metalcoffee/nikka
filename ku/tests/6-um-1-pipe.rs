#![deny(warnings)]
#![feature(allocator_api)]

use std::{
    fmt,
    thread,
    time::{
        Duration,
        Instant,
    },
    vec::Vec,
};

use rand::{
    Rng,
    SeedableRng,
    distributions::Alphanumeric,
    rngs::SmallRng,
};
use rstest::rstest;

use ku::{
    error::{
        Error,
        Result,
    },
    ipc::pipe::{
        self,
        Error::Overflow,
        ReadBuffer,
        RingBuffer,
        RingBufferWriteTx,
        Tag,
        WriteBuffer,
    },
    log::{
        debug,
        trace,
    },
    memory::{
        Page,
        size::{
            MiB,
            Size,
        },
    },
};

use allocator::BigForPipe;

mod allocator;
mod log;

const QUALITY: Quality = Quality::Paranoid;
const SEED: u64 = 314159265;
const WINDOW_DURATION: Duration = Duration::from_millis(
    if cfg!(debug_assertions) {
        500
    } else {
        50
    },
);

#[rstest]
#[cfg_attr(not(miri), timeout(Duration::from_secs(1)))]
fn check_continuous_mapping() {
    let frame_count = 4;
    let mut allocator = BigForPipe::new(false);

    let (_, write_buffer) = pipe::make(frame_count, &mut allocator).unwrap();
    let block = write_buffer.block();

    debug!(ring_buffer_block = %block);
    assert_eq!(block.start_address().into_usize() % Page::SIZE, 0);
    assert_eq!(block.size() % (2 * Page::SIZE), 0);
    assert!(block.size() > 2 * Page::SIZE);
    assert!(frame_count * Page::SIZE - 16 < write_buffer.max_capacity());
    assert!(write_buffer.max_capacity() < frame_count * Page::SIZE - 1);

    let mut read_rng = SmallRng::seed_from_u64(SEED);
    let mut write_rng = read_rng.clone();

    let buf = unsafe { block.try_into_mut_slice::<u8>().unwrap() };
    let (read_buf, write_buf) = buf.split_at_mut(block.size() / 2);

    for write_ref in write_buf {
        let value = write_rng.r#gen();
        let write_ptr = write_ref as *mut u8;
        unsafe {
            write_ptr.write_volatile(value);
        }
    }

    for read_ref in read_buf {
        let value = read_rng.r#gen();
        let read_ptr = read_ref as *const u8;
        assert_eq!(
            unsafe { read_ptr.read_volatile() },
            value,
            "RingBuffer is not mapped properly to be continuous",
        );
    }

    allocator.unmap();
}

#[rstest]
#[case(false, false, false)]
#[case(false, false, true)]
#[case(false, true, false)]
#[case(false, true, true)]
#[case(true, false, false)]
#[case(true, false, true)]
#[case(true, true, false)]
#[case(true, true, true)]
#[cfg_attr(not(miri), timeout(Duration::from_secs(1)))]
fn reader_close(
    #[case] corrupt: bool,
    #[case] close_first: bool,
    #[case] aborted_read_tx: bool,
) {
    debug!(corrupt, close_first, aborted_read_tx);

    let mut allocator = BigForPipe::new(false);

    let (mut read_buffer, mut write_buffer) = pipe::make(4, &mut allocator).unwrap();
    let chunk = [1, 2, 3];

    if aborted_read_tx {
        let mut write_tx = write_buffer.write_tx().unwrap();
        write_tx.write(&chunk).unwrap();
        write_tx.commit();

        let mut read_tx = read_buffer.read_tx().unwrap();
        unsafe {
            assert_eq!(read_tx.read().unwrap(), chunk);
            assert!(read_tx.read().is_none());
        }

        drop(read_tx);
    }

    let mut write_tx = write_buffer.write_tx().unwrap();
    write_tx.write(&chunk).unwrap();

    if close_first {
        close(read_buffer, corrupt);
        write_tx.commit();
    } else {
        write_tx.commit();
        close(read_buffer, corrupt);
    }

    assert!(write_buffer.write_tx().is_none());

    allocator.unmap();
}

#[rstest]
#[case(false)]
#[case(true)]
#[cfg_attr(not(miri), timeout(Duration::from_secs(1)))]
fn writer_close(#[case] corrupt: bool) {
    debug!(corrupt);

    let mut allocator = BigForPipe::new(false);

    let (mut read_buffer, mut write_buffer) = pipe::make(4, &mut allocator).unwrap();
    let chunk = [1, 2, 3];

    let mut write_tx = write_buffer.write_tx().unwrap();
    write_tx.write(&chunk).unwrap();
    write_tx.commit();

    close(write_buffer, corrupt);

    if let Some(mut read_tx) = read_buffer.read_tx() {
        assert!(!corrupt);

        unsafe {
            assert_eq!(read_tx.read().unwrap(), chunk);
            assert!(read_tx.read().is_none());
        }

        read_tx.commit();
    } else {
        assert!(corrupt);
    }

    assert!(read_buffer.read_tx().is_none());

    allocator.unmap();
}

#[rstest]
#[cfg_attr(not(miri), timeout(Duration::from_secs(60)))]
fn sequential() {
    let mut allocator = BigForPipe::new(false);

    let (mut read_buffer, mut write_buffer) = pipe::make(4, &mut allocator).unwrap();

    let mut rng = SmallRng::seed_from_u64(SEED);
    let mut read_ops = Generator::new(write_buffer.max_capacity(), QUALITY, SEED);
    let mut write_ops = read_ops.clone();
    let mut write_errors = 0;

    for iteration in 0 .. 10_000 {
        if iteration % 1_000 == 0 {
            debug!(
                iteration,
                read_stats = ?read_buffer.read_stats(),
                write_stats = ?write_buffer.write_stats(),
            );
        }

        if rng.gen_ratio(3, 4) {
            write_errors += write_with_retries(
                &mut read_buffer,
                &mut write_buffer,
                &mut write_ops,
                &mut read_ops,
                &mut rng,
            );
        } else {
            read(&mut read_buffer, &mut read_ops, &mut rng);
        }
    }

    allocator.unmap();

    assert!(
        write_errors > 0,
        "the test does not check for RingBuffer overflow",
    );
}

#[rstest]
#[cfg_attr(not(miri), timeout(Duration::from_secs(300)))]
fn concurrent() {
    const ITERATIONS: u64 = if cfg!(debug_assertions) {
        20
    } else {
        100
    };

    for iteration in 0 .. ITERATIONS {
        let mut allocator = BigForPipe::new(false);

        let (read_buffer, write_buffer) = pipe::make(4, &mut allocator).unwrap();

        let read_thread = thread::Builder::new()
            .name("read_thread".to_string())
            .spawn(move || read_thread(iteration, SEED + iteration, read_buffer))
            .unwrap();

        let write_thread = thread::Builder::new()
            .name("write_thread".to_string())
            .spawn(move || write_thread(iteration, SEED + iteration, write_buffer))
            .unwrap();

        assert!(read_thread.join().is_ok());
        assert!(write_thread.join().is_ok());

        allocator.unmap();
    }
}

fn close<T: Tag>(
    mut buffer: RingBuffer<T>,
    corrupt: bool,
) {
    if corrupt {
        unsafe {
            buffer.block().try_into_mut_slice::<u8>().unwrap().fill(0xA5);
        }
    } else {
        buffer.close();
    }
}

fn read_thread(
    iteration: u64,
    seed: u64,
    mut read_buffer: ReadBuffer,
) {
    let mut rng = SmallRng::seed_from_u64(seed);
    let mut read_ops = Generator::new(read_buffer.max_capacity(), QUALITY, seed);

    let mut read_tx = 0;
    let mut window_bytes = 0;
    let mut window_begin = Instant::now();

    while let Some(bytes) = read(&mut read_buffer, &mut read_ops, &mut rng) {
        window_bytes += bytes;

        let elapsed = window_begin.elapsed();
        if elapsed >= WINDOW_DURATION {
            let throughput_per_second =
                u128::try_from(window_bytes).unwrap() * 1_000_000 / elapsed.as_micros();
            let throughput_per_second = throughput_per_second.try_into().unwrap();
            let throughput = Size::bytes(throughput_per_second);
            debug!(
                iteration,
                read_tx,
                throughput_per_second = %throughput,
                window_bytes,
                read_stats = ?read_buffer.read_stats(),
            );

            let min_throughput = if cfg!(debug_assertions) {
                MiB
            } else {
                500 * MiB
            };
            assert!(
                throughput_per_second > min_throughput,
                "the ring buffer is too slow, throughput={throughput}/s",
            );

            window_begin = Instant::now();
            window_bytes = 0;
        }

        if bytes == 0 {
            thread::yield_now();
        }

        read_tx += 1;
    }
}

fn write_thread(
    iteration: u64,
    seed: u64,
    mut write_buffer: WriteBuffer,
) {
    let mut rng = SmallRng::seed_from_u64(seed);
    let mut write_ops = Generator::new(write_buffer.max_capacity(), QUALITY, seed);

    let mut window_begin = Instant::now();

    const WRITE_TRANSACTIONS: u64 = if cfg!(debug_assertions) {
        10_000
    } else {
        100_000
    };

    for write_tx in 0 .. WRITE_TRANSACTIONS {
        let operation = write_ops.generate_operation();

        if window_begin.elapsed() >= WINDOW_DURATION {
            debug!(iteration, write_tx, write_stats = ?write_buffer.write_stats());
            window_begin = Instant::now();
        }

        while let Err(write_error) = write(&mut write_buffer, &operation, &mut rng) {
            trace!(%operation, ?write_error);
            thread::yield_now();
        }
    }

    write_buffer.close();
}

fn read(
    ring_buffer: &mut ReadBuffer,
    read_ops: &mut Generator,
    rng: &mut SmallRng,
) -> Option<usize> {
    let mut tx = ring_buffer.read_tx()?;
    let mut total_bytes = 0;

    let commit = rng.gen_ratio(1, 2);

    while let Some(data) = unsafe { tx.read() } {
        match read_ops.quality {
            Quality::Debuggable => trace!(
                data = %RunLengthEncoding(data),
                len = data.len(),
                "read_tx",
            ),
            Quality::Paranoid => trace!(?data, len = data.len(), "read_tx"),
        }

        if commit {
            check_read_tx(data, read_ops);
        }

        total_bytes += data.len();
    }

    if commit {
        trace!("read commit");
        tx.commit();
    }

    Some(total_bytes)
}

fn check_read_tx(
    mut data: &[u8],
    read_ops: &mut Generator,
) {
    let mut count = 0;

    while !data.is_empty() {
        let expected = read_ops.generate_operation();
        trace!(data = %RunLengthEncoding(data), operation = %expected, "read");

        if expected.commit {
            let (got, more_data) = data.split_at(expected.data.len());

            // The main point of the stress test ---
            // check that read yields the same data that was written.
            assert_eq!(*got, *expected.data);

            data = more_data;
            count += 1;
        }
    }

    if count > 1 {
        trace!(count, "multiple write transactions in one read transaction");
    }
}

fn write_with_retries(
    read_buffer: &mut ReadBuffer,
    write_buffer: &mut WriteBuffer,
    write_ops: &mut Generator,
    read_ops: &mut Generator,
    rng: &mut SmallRng,
) -> usize {
    let operation = write_ops.generate_operation();
    let mut write_errors = 0;

    trace!(%operation, "starting a chunked write operation");

    while let Err(write_error) = write(write_buffer, &operation, rng) {
        trace!(%operation, ?write_error);

        write_errors += 1;
        assert!(write_errors < 1_000);

        if let Error::Pipe(Overflow {
            capacity,
            len,
            exceeding_object_len,
        }) = write_error
        {
            if capacity > 0 && len + exceeding_object_len <= capacity {
                debug!(%operation, ?write_error, "unexpected capacity overflow");
            }
            assert!(capacity == 0 || len + exceeding_object_len > capacity);
        } else {
            panic!("unexpected error {write_error:?}");
        }

        read(read_buffer, read_ops, rng).unwrap();
    }

    write_errors
}

fn write(
    ring_buffer: &mut WriteBuffer,
    operation: &Operation,
    rng: &mut SmallRng,
) -> Result<()> {
    let mut tx = ring_buffer.write_tx().unwrap();

    write_chunked(&mut tx, operation, rng)?;

    if operation.commit {
        tx.commit();
    }

    Ok(())
}

fn write_chunked(
    tx: &mut RingBufferWriteTx,
    operation: &Operation,
    rng: &mut SmallRng,
) -> Result<()> {
    let mut chunk_count = 0;
    let mut data = operation.data;

    loop {
        let old_capacity = tx.capacity();
        let (chunk, more_data) = data.split_at(rng.gen_range(0 ..= data.len()));

        tx.write(chunk)?;

        assert!(tx.capacity() + chunk.len() >= old_capacity);

        data = more_data;
        chunk_count += 1;
        if data.is_empty() {
            break;
        }
    }

    trace!(%operation, chunk_count, "write");

    Ok(())
}

#[derive(Clone)]
struct Generator {
    buffer: Vec<u8>,
    quality: Quality,
    rng: SmallRng,
}

impl Generator {
    fn new(
        max_capacity: usize,
        quality: Quality,
        seed: u64,
    ) -> Self {
        Self {
            buffer: (0 .. max_capacity).map(|_| 0).collect(),
            quality,
            rng: SmallRng::seed_from_u64(seed),
        }
    }

    fn generate_operation(&mut self) -> Operation<'_> {
        Operation::new(&mut self.buffer, self.quality, &mut self.rng)
    }
}

struct Operation<'a> {
    commit: bool,
    data: &'a [u8],
    quality: Quality,
}

impl<'a> Operation<'a> {
    fn new(
        data: &'a mut [u8],
        quality: Quality,
        rng: &mut SmallRng,
    ) -> Self {
        Self {
            commit: rng.gen_ratio(3, 4),
            data: Self::fill(data, quality, rng),
            quality,
        }
    }

    fn fill<'b>(
        data: &'b mut [u8],
        quality: Quality,
        rng: &mut SmallRng,
    ) -> &'b [u8] {
        let len = Self::generate_len(data.len(), rng);

        match quality {
            Quality::Debuggable => data[.. len].fill(rng.sample(Alphanumeric)),
            Quality::Paranoid => rng.fill(&mut data[.. len]),
        }

        &data[.. len]
    }

    fn generate_len(
        max_len: usize,
        rng: &mut SmallRng,
    ) -> usize {
        const BOUNDARY_SIZE: usize = 16;

        if rng.gen_ratio(3, 4) {
            rng.gen_range(BOUNDARY_SIZE .. max_len - BOUNDARY_SIZE + 1)
        } else if rng.gen_ratio(1, 2) {
            rng.gen_range(0 .. BOUNDARY_SIZE)
        } else {
            rng.gen_range(max_len - BOUNDARY_SIZE + 1 ..= max_len)
        }
    }
}

impl fmt::Display for Operation<'_> {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        match self.quality {
            Quality::Debuggable => write!(
                formatter,
                "{:?} x {}, {}",
                if self.data.is_empty() {
                    ' '
                } else {
                    self.data[0] as char
                },
                self.data.len(),
                if self.commit {
                    "commit"
                } else {
                    "rollback"
                },
            ),
            Quality::Paranoid => write!(
                formatter,
                "{:?}, {}",
                self.data,
                if self.commit {
                    "commit"
                } else {
                    "rollback"
                },
            ),
        }
    }
}

struct RunLengthEncoding<'a>(&'a [u8]);

impl fmt::Display for RunLengthEncoding<'_> {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        let mut block_count = 0;
        let mut total_len = 0;

        for block in self.0.chunk_by(|a, b| a == b) {
            write!(formatter, "{:?} x {}, ", block[0] as char, block.len())?;
            block_count += 1;
            total_len += block.len();
        }

        write!(
            formatter,
            "block_count = {block_count}, total_len = {total_len}",
        )
    }
}

#[allow(unused)]
#[derive(Clone, Copy, Debug)]
enum Quality {
    Debuggable,
    Paranoid,
}

#[ctor::ctor]
fn init() {
    log::init();
}
