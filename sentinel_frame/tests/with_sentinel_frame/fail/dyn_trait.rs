use sentinel_frame::with_sentinel_frame;

trait ObjectSafeTrait {
    fn do_something(&self);
}

#[with_sentinel_frame]
fn dyn_trait_ref(v: &dyn ObjectSafeTrait) {
    v.do_something();
}

#[with_sentinel_frame]
fn dyn_trait_mut_ref(v: &mut dyn ObjectSafeTrait) {
    v.do_something();
}

#[with_sentinel_frame]
fn dyn_trait_ptr(v: *const dyn ObjectSafeTrait) {
    unsafe {
        (*v).do_someshing();
    }
}

#[with_sentinel_frame]
fn dyn_trait_mut_ptr(v: *mut dyn ObjectSafeTrait) {
    unsafe {
        (*v).do_someshing();
    }
}

fn main() {
}
