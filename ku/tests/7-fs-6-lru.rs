#![deny(warnings)]

use std::{
    collections::HashMap,
    time::Duration,
};

use rand::{
    Rng,
    SeedableRng,
    rngs::SmallRng,
};
use rstest::rstest;

use ku::{
    collections::Lru,
    log::debug,
};

mod log;

#[rstest]
#[timeout(Duration::from_secs(1))]
fn basic_map() {
    let size = 5;
    let mut lru = Lru::new(size);

    for key in 0 .. size {
        assert!(lru.insert(key, 2 * key).is_none());
        lru.validate();
    }

    debug!(%lru);

    for key in 0 .. size {
        assert_eq!(lru.remove(&key), Some((key, 2 * key)));
        lru.validate();

        assert_eq!(lru.remove(&key), None);
        lru.validate();

        assert!(lru.insert(key, 3 * key).is_none());
        lru.validate();
    }

    debug!(%lru);
}

#[rstest]
#[timeout(Duration::from_secs(1))]
fn basic_lru() {
    let size = 5;
    let mut lru = Lru::new(size);

    for key in 0 .. size {
        assert!(lru.insert(key, 2 * key).is_none());
        lru.validate();
    }

    debug!(%lru);

    for evictee in 0 .. size {
        let key = size + evictee;
        assert_eq!(lru.insert(key, 2 * key), Some((evictee, 2 * evictee)));
        lru.validate();
    }

    debug!(%lru);

    for key in (size .. (2 * size)).rev() {
        assert_eq!(lru.remove(&key), Some((key, 2 * key)));
        lru.validate();

        assert_eq!(lru.remove(&key), None);
        lru.validate();

        assert!(lru.insert(key, 3 * key).is_none());
        lru.validate();
    }

    debug!(%lru);

    for evictee in (size .. (2 * size)).rev() {
        let key = size + evictee;
        assert_eq!(lru.insert(key, 2 * key), Some((evictee, 3 * evictee)));
        lru.validate();
    }

    debug!(%lru);
}

#[rstest]
#[timeout(Duration::from_secs(60))]
fn stress() {
    let size = 100;
    let mut lru = Lru::new(size);
    let mut naive_lru = HashMap::new();
    let mut rng = SmallRng::seed_from_u64(SEED);
    let mut evictions = 0;

    for iteration in 0 .. 200_000 {
        let key = rng.gen_range(0 .. 2 * size);
        let value = 2 * key;

        if rng.gen_ratio(1, 2) {
            let evictee = lru.insert(key, value);
            lru.validate();

            naive_lru.insert(key, (value, iteration));

            let mut naive_evictee = None;
            let mut naive_evictee_iteration = iteration;
            if naive_lru.len() > size {
                for (k, (v, i)) in naive_lru.iter() {
                    if *i < naive_evictee_iteration {
                        naive_evictee_iteration = *i;
                        naive_evictee = Some((*k, *v));
                    }
                }
            }

            if let Some((key, _)) = naive_evictee {
                naive_lru.remove(&key);
            }

            if evictee.is_some() {
                evictions += 1;
            }
        } else {
            let value = lru.remove(&key);
            lru.validate();

            let naive_value = naive_lru.get(&key).map(|(v, _)| (key, *v));
            naive_lru.remove(&key);

            assert_eq!(value, naive_value);
        }
    }

    assert!(evictions > 1_000);
}

const SEED: u64 = 314159265;

#[ctor::ctor]
fn init() {
    log::init();
}
