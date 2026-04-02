use std::{os::raw::c_void, ptr::NonNull};

type DeleteFromVoid = unsafe fn(NonNull<c_void>);

/// A pair of some type-erased user data and a deleter function.
/// This is used when setting mpv callbacks where mpv holds onto the user data,
/// but we need to ensure that data is valid for the lifetime of the handle/render context.
pub struct UnsafeErasedBox {
    data: NonNull<c_void>,
    deleter: DeleteFromVoid,
}
impl UnsafeErasedBox {
    /// Places `T` on the heap with the global allocator and then makes an `UnsafeErasedBox` out of it
    pub fn new<T>(data: T) -> Self {
        let data =
            unsafe { NonNull::new_unchecked(Box::into_raw(Box::new(data)).cast::<c_void>()) };
        let deleter = dropper::<T>;
        Self { data, deleter }
    }
    /// Gets the user data from the erased box.
    pub fn user_data(&self) -> NonNull<c_void> {
        self.data
    }
}

impl Drop for UnsafeErasedBox {
    fn drop(&mut self) {
        unsafe { (self.deleter)(self.data) }
    }
}

unsafe fn dropper<T>(data: NonNull<c_void>) {
    unsafe {
        _ = Box::from_raw(data.cast::<T>().as_ptr());
    }
}

macro_rules! gen_trampolines {
    ($(pub trampoline fn $fun:ident($($args:ident : $ts:ident),*) = $tramp_name:ident;)*) => {
        $(
            unsafe extern "C" fn $tramp_name<F, $($ts,)* O>(ptr: *mut c_void, $($args : $ts),*) -> O
            where
                F: FnMut($($ts),*) -> O
            {
                unsafe { ptr.cast::<F>().as_mut() }.expect("mpv passed a null pointer where the user data was expected")($($args),*)
            }
            pub fn $fun<F, $($ts,)* O>(func: F) -> (unsafe extern "C" fn(*mut c_void, $($ts),*) -> O, UnsafeErasedBox)
            where
                F: FnMut($($ts),*) -> O {
                ( $tramp_name::<F, $($ts,)* O>, UnsafeErasedBox::new(func))
            }
        )*
    };
}
gen_trampolines! {
    pub trampoline fn owned_trampoline_0() = trampoline0;
    pub trampoline fn owned_trampoline_1(arg0: T) = trampoline1;
    // pub trampoline fn owned_trampoline_2(arg0: T, arg1: U) = trampoline2;
}
