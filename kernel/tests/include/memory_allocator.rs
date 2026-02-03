fn memory_allocator_basic() {
    let start_info = allocator::info();
    if Info::IS_SUPPORTED {
        debug!(%start_info);
    }

    {
        let mut a = Box::new(1_i64);
        let a_ptr = &mut *a as *mut i64;
        unsafe {
            a_ptr.write_volatile(a_ptr.read_volatile() + 1);
        }
        debug!(box_contents = *a);
        my_assert!(*a == 2);

        if Info::IS_SUPPORTED {
            let info = allocator::info();
            let requested = mem::size_of_val(&*a);
            let info_diff = (info - start_info).unwrap();

            debug!(%info);
            debug!(%info_diff);

            my_assert!(info_diff.allocations().positive() == 1);
            my_assert!(info_diff.allocations().negative() == 0);
            my_assert!(info_diff.allocated().balance() >= requested);
            my_assert!(info.pages().positive() > 0);
            my_assert!(info.pages().balance() > 0);
        }
    }

    if Info::IS_SUPPORTED {
        let end_info = allocator::info();
        let end_info_diff = (end_info - start_info).unwrap();
        debug!(%end_info);
        debug!(%end_info_diff);

        my_assert!(end_info_diff.allocations().positive() == 1);
        my_assert!(end_info_diff.allocations().negative() == 1);
        my_assert!(end_info_diff.requested().balance() == 0);
        my_assert!(end_info_diff.allocated().balance() == 0);
        my_assert!(end_info.pages().positive() > 0);
        my_assert!(end_info_diff.fragmentation_loss() < MiB);
    }
}

fn memory_allocator_alignment() {
    let max_lb_align = 12;

    for lb_align in 0 ..= max_lb_align {
        let align = 1 << lb_align;
        debug!(align);

        for lb_size in 6 ..= (max_lb_align + 2) {
            let min_size = (1_usize << lb_size).saturating_sub(10) + 1;
            let max_size = (1 << lb_size) + 10;

            for size in min_size ..= max_size {
                let layout = Layout::from_size_align(size, align).unwrap();
                let ptr = Global.allocate(layout).unwrap();

                assert!(ptr.len() >= size);
                assert_eq!(ptr.addr().get() % align, 0);

                unsafe { Global.deallocate(ptr.as_non_null_ptr(), layout) };
            }
        }
    }
}

fn memory_allocator_grow_and_shrink() {
    let start_info = allocator::info();
    if Info::IS_SUPPORTED {
        debug!(%start_info);
    }

    let mut vec = Vec::new();
    let mut push_sum = 0;

    for a in 1 .. 3 * Page::SIZE {
        vec.push(a);
        my_assert!(vec.len() == a);
        push_sum += a;
    }

    let contents_sum = vec.iter().sum::<usize>();
    debug!(contents_sum, push_sum);
    my_assert!(contents_sum == push_sum);

    if Info::IS_SUPPORTED {
        let info = allocator::info();
        let info_diff = (info - start_info).unwrap();
        debug!(%info);
        debug!(%info_diff);
        my_assert!(info_diff.fragmentation_loss() < MiB);
    }

    let mut pop_sum = 0;

    while !vec.is_empty() {
        pop_sum += vec.pop().unwrap();
        if vec.len() <= vec.capacity() / 2 {
            vec.shrink_to_fit();
        }
    }

    debug!(contents_sum, pop_sum);
    my_assert!(contents_sum == pop_sum);

    if Info::IS_SUPPORTED {
        let end_info = allocator::info();
        let end_info_diff = (end_info - start_info).unwrap();
        debug!(%end_info);
        debug!(%end_info_diff);
        my_assert!(end_info_diff.allocations().balance() == 0);
        my_assert!(end_info_diff.fragmentation_loss() < MiB);
    }
}

fn memory_allocator_stress(
    values: usize,
    max_fragmentation_loss: fn(usize) -> usize,
) -> usize {
    let start_info = allocator::info();
    if Info::IS_SUPPORTED {
        debug!(%start_info);
    }

    let mut vec = Vec::new();
    let mut push_sum = 0;

    let mut pages = BTreeMap::<usize, usize>::new();

    for a in 0 .. values {
        let b = Box::new(a * a);
        let page_index = Page::containing(Virt::from_ref(b.as_ref())).index();
        pages.entry(page_index).and_modify(|count| *count += 1).or_insert(1);
        vec.push(b);
        my_assert!(vec.len() == a + 1);
        push_sum += a * a;

        if Info::IS_SUPPORTED {
            let current_info_diff = (allocator::info() - start_info).unwrap();
            let fragmentation_loss = Size::bytes(current_info_diff.fragmentation_loss());
            let max_fragmentation_loss = Size::bytes(max_fragmentation_loss(vec.len()));
            if vec.len() % (values / 20) == 0 || fragmentation_loss > max_fragmentation_loss {
                debug!(vector_length = vec.len(), %fragmentation_loss, %max_fragmentation_loss);
            }
            my_assert!(fragmentation_loss <= max_fragmentation_loss);
        }
    }

    let contents_sum = vec.iter().map(|x| **x).sum::<usize>();
    debug!(contents_sum, push_sum);
    my_assert!(contents_sum == push_sum);

    if Info::IS_SUPPORTED {
        let info = allocator::info();
        let info_diff = (info - start_info).unwrap();
        debug!(%info);
        debug!(%info_diff);
    }

    let mut pop_sum = 0;

    while !vec.is_empty() {
        pop_sum += *vec.pop().unwrap();
        if vec.len() <= vec.capacity() / 2 {
            vec.shrink_to_fit();
        }

        if Info::IS_SUPPORTED {
            let current_info_diff = (allocator::info() - start_info).unwrap();
            let fragmentation_loss = Size::bytes(current_info_diff.fragmentation_loss());
            let max_fragmentation_loss = Size::bytes(max_fragmentation_loss(vec.len()));
            if vec.len() % (values / 20) == 0 || fragmentation_loss > max_fragmentation_loss {
                debug!(vector_length = vec.len(), %fragmentation_loss, %max_fragmentation_loss);
            }
            my_assert!(fragmentation_loss <= max_fragmentation_loss);
        }
    }

    debug!(contents_sum, pop_sum);
    my_assert!(contents_sum == pop_sum);

    let pages_for_values = pages.len();
    drop(pages);

    if Info::IS_SUPPORTED {
        let end_info = allocator::info();
        let end_info_diff = (end_info - start_info).unwrap();
        let fragmentation_loss = Size::bytes(end_info_diff.fragmentation_loss());
        let max_fragmentation_loss = Size::bytes(max_fragmentation_loss(vec.len()));
        debug!(%end_info);
        debug!(%end_info_diff);
        debug!(vector_length = vec.len(), %fragmentation_loss, %max_fragmentation_loss);
        my_assert!(end_info_diff.allocations().balance() == 0);
        my_assert!(fragmentation_loss <= max_fragmentation_loss);
    }

    pages_for_values
}
