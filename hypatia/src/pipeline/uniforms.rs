use std::alloc::Layout;
use std::ffi::CString;
use std::marker::PhantomData;

use std::any::{Any, TypeId};

use std::ptr::NonNull;

use bitvec::vec::BitVec;
use bytemuck::Pod;
use error_set::error_set;
use eyre::Context;
use indexmap::IndexMap;
use mini_gl_bindings::gl::types::GLuint;
use mini_gl_bindings::{GlCtx, Program, Uniform, UniformLocation};
use rustc_hash::FxBuildHasher;

error_set! {
    StoreError := {
        #[display("Cannot access uniform slot of type {expected} with the wrong type")]
        WrongType {  expected: &'static str }
    }
}

#[repr(C)]
struct UniformSlot<T> {
    current: T,
    ids: [GLuint],
}
impl<T: Pod + Uniform + Any> UniformSlot<T> {
    fn zeroed(size: usize) -> Box<Self> {
        const {
            assert!(size_of::<T>() != 0);
        };
        let (layout, _) = Layout::new::<T>()
            .extend(Layout::array::<GLuint>(size).unwrap())
            .unwrap();
        unsafe {
            // SAFETY: we know that the layout is not zst
            let ptr = std::alloc::alloc_zeroed(layout);
            if ptr.is_null() {
                std::alloc::handle_alloc_error(layout);
            }
            let dst_ptr =
                std::ptr::slice_from_raw_parts_mut(ptr.cast::<GLuint>(), size) as *mut Self;
            Box::from_raw(dst_ptr)
        }
    }

    fn erase(self: Box<Self>) -> ErasedUniformSlot {
        ErasedUniformSlot {
            ptr: NonNull::new(Box::into_raw(self).cast()).unwrap(),
            vtable: static_uniform_slot_vtable_for::<T>(),
        }
    }
    pub fn current(&mut self) -> &mut T {
        &mut self.current
    }
}

struct ErasedUniformSlotVTable {
    store: unsafe fn(this: *mut (), value: *const (), type_id: TypeId) -> Result<bool, StoreError>,
    flush: unsafe fn(this: *const (), idx: usize, num_slots: usize, cx: &GlCtx),
    drop: unsafe fn(this: *mut (), num_slots: usize),
}
/// Stores data into this slot if needed, returning whether the write actually did anything
/// or an error if the types do not match.
unsafe fn store_fn_for<T: Uniform>(
    this: *mut (),
    value: *const (),
    type_id: TypeId,
) -> Result<bool, StoreError> {
    if TypeId::of::<T>() == type_id {
        unsafe {
            let current_value = &mut *this.cast::<T>();
            let new_value = &*value.cast::<T>();
            // value is the same as before, don't store
            if current_value == new_value {
                return Ok(false);
            }
            *current_value = *new_value;
        };
        Ok(true)
    } else {
        Err(StoreError::WrongType {
            expected: std::any::type_name::<T>(),
        })
    }
}

unsafe fn flush_fn_for<T: Uniform>(this: *const (), idx: usize, num_slots: usize, cx: &GlCtx) {
    unsafe {
        let uniform_slot_ptr =
            std::ptr::slice_from_raw_parts(this.cast::<GLuint>(), num_slots) as *mut UniformSlot<T>;
        let data = &(*uniform_slot_ptr).current;
        let ids = &(*uniform_slot_ptr).ids;
        if ids[idx] != GLuint::MAX {
            data.write_to_location(cx, ids[idx]);
        }
    };
}
unsafe fn drop_for<T: Uniform>(this: *mut (), num_items: usize) {
    unsafe {
        let dst_ptr = std::ptr::slice_from_raw_parts_mut(this.cast::<GLuint>(), num_items)
            as *mut UniformSlot<T>;
        _ = Box::from_raw(dst_ptr);
    }
}

const fn static_uniform_slot_vtable_for<T: Uniform>() -> &'static ErasedUniformSlotVTable {
    &ErasedUniformSlotVTable {
        store: store_fn_for::<T>,
        flush: flush_fn_for::<T>,
        drop: drop_for::<T>,
    }
}

struct ErasedUniformSlot {
    ptr: NonNull<()>,
    vtable: &'static ErasedUniformSlotVTable,
}
impl ErasedUniformSlot {
    fn store(&mut self, value: &dyn Any) -> Result<bool, StoreError> {
        unsafe {
            (self.vtable.store)(
                self.ptr.as_ptr(),
                std::ptr::from_ref(value).cast::<()>(),
                value.type_id(),
            )
        }
    }
    fn flush(&self, idx: usize, num_slots: usize, cx: &GlCtx) {
        unsafe { (self.vtable.flush)(self.ptr.as_ptr(), idx, num_slots, cx) }
    }
    unsafe fn drop_from_count(&mut self, count: usize) {
        unsafe { (self.vtable.drop)(self.ptr.as_ptr(), count) }
    }
}
#[derive(Debug)]
pub struct SlotHandle<T: ?Sized>(u16, PhantomData<T>);

impl<T: ?Sized> SlotHandle<T> {
    /// Checks whether the handle is the null handle, ie. does not point to a slot because it's unused.
    /// A null handle can be used just like any other slot handle except that any writes to it will be a
    /// noop
    pub fn is_null(&self) -> bool {
        self.0 == NULL_HANDLE
    }
}
impl<T> Clone for SlotHandle<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for SlotHandle<T> {}
/// Global storage for uniforms across multiple programs.
// The reason for this abstraction is that Hypatia will eventually receive support
// for custom plugins that allow changing uniforms on demand, but I don't know what approach
// I want to take yet
pub struct UniformStorage {
    dirty: BitVec,
    map: IndexMap<CString, ErasedUniformSlot, FxBuildHasher>,
    num_programs: usize,
    is_dirty: bool,
}
const NULL_HANDLE: u16 = u16::MAX;
impl UniformStorage {
    pub fn new(num_programs: usize) -> Self {
        Self {
            dirty: BitVec::new(),
            map: <_>::default(),
            num_programs,
            is_dirty: false,
        }
    }

    /// Returns a handle to data held by the uniform with the name specified in `name`.
    /// Note that this function does not fail for invalid names or names used by no programs;
    /// it only fails when the slot index overflows. If the name is used by no programs whatsoever,
    /// then returns a null slot.
    pub fn slot<'a, T: Uniform>(
        &mut self,
        name: CString,
        programs: impl Iterator<Item = &'a Program>,
    ) -> eyre::Result<SlotHandle<T>> {
        let len = self.map.len();
        let entry = self.map.entry(name.clone());
        let index = entry.index();
        let mut slot_should_be_null = true;
        let was_occupied = entry.index() != len;
        entry.or_insert_with(|| {
            let mut zeroed = UniformSlot::<T>::zeroed(self.num_programs);
            for (slot, program) in zeroed.ids.iter_mut().zip(programs) {
                *slot = program
                    .uniform_location::<T>(&name)
                    .ok()
                    .as_ref()
                    .map(UniformLocation::id)
                    .unwrap_or(GLuint::MAX);
                slot_should_be_null &= *slot == GLuint::MAX;
            }
            self.dirty.push(false);
            zeroed.erase()
        });
        if !was_occupied && slot_should_be_null {
            self.map.pop();
            self.dirty.pop();
            return Ok(SlotHandle(NULL_HANDLE, PhantomData));
        }
        let idx = index
            .try_into()
            .context("Overflowed the 16-bit uniform slot handle size")?;
        if idx == NULL_HANDLE {
            return Err(eyre::eyre!(
                "Overflowed the 16-bit uniform slot handle size"
            ));
        }
        Ok(SlotHandle(idx, PhantomData))
    }
    /// Writes data to a typed slot, ready to later be flushed during rendering. Writing to null and invalid slots are treated
    /// treated as noops.
    pub fn write_slot<T: Uniform + Any + Pod>(&mut self, slot: SlotHandle<T>, data: &T) {
        self.write_slot_erased(slot.0, data).unwrap();
    }
    /// Tries to write erased data to a slot. This function fails if the slot does not have the correct type of data.
    /// Does no action if this slot is not valid or if the handle is null.
    pub fn write_slot_erased(&mut self, slot: u16, data: &dyn Any) -> Result<(), StoreError> {
        if slot == NULL_HANDLE {
            return Ok(());
        }
        if let Some((_, entry)) = self.map.get_index_mut(slot as usize)
            && entry.store(data)?
        {
            self.dirty.set(slot as usize, true);
            self.is_dirty = true
        }

        Ok(())
    }
    /// Flushes all of the dirty uniforms related to the nth program if n is in bounds. This function assumes
    /// that the specified program is currently in use. Does not mark the slots as clean after.
    pub fn flush_nth(&self, n: usize, ctx: &GlCtx) {
        if !self.is_dirty {
            return;
        }
        for idx in self.dirty.iter_ones() {
            if let Some((_, slot)) = self.map.get_index(idx) {
                slot.flush(n, self.num_programs, ctx);
            }
        }
    }
    /// Finishes the flush operation by marking all of the slots as clean
    pub fn finish_flush(&mut self) {
        self.dirty.fill(false);
        self.is_dirty = false;
    }
    pub fn is_dirty(&self) -> bool {
        self.is_dirty
    }
}

impl Drop for UniformStorage {
    fn drop(&mut self) {
        let map = std::mem::take(&mut self.map);
        for (_, mut v) in map {
            unsafe { v.drop_from_count(self.num_programs) };
        }
    }
}
