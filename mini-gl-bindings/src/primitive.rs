use crate::primitive::sealed::SealedPrimitive;
use crate::vertex::{ComponentCount, VertexComponent};

use bytemuck::{Pod, Zeroable};
// use epoxy::*;
use crate::gl::*;
use glam::*;

mod sealed {
    use crate::vertex::VertexComponent;
    pub trait SealedPrimitive: bytemuck::Pod {
        #[doc(hidden)]
        const COMPONENT: VertexComponent;
    }
}

/// The kind of data type a vertex component has
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum DataType {
    Float = FLOAT,
    // Double = DOUBLE,
    SignedInt = INT,
    UnsignedInt = UNSIGNED_INT,
    Byte = BYTE,
    UnsignedByte = UNSIGNED_BYTE,
    Short = SHORT,
    UnsignedShort = UNSIGNED_SHORT,
}

/// An OpenGL primitive that can be stored and used as components of a vertex.
///
/// This trait represents the same invariants as [`Pod`] with the additional restriction that the type is
/// supported as a vertex component.
pub trait GLPrimitive: sealed::SealedPrimitive {}

macro_rules! impl_prim_for_prim {
    ($($ty:ident => $prim:ident;)*) => {
        $(impl sealed::SealedPrimitive for $ty {
            const COMPONENT: VertexComponent = VertexComponent {
                ty: DataType::$prim,
                count: ComponentCount::_1,
                normalized: false
            };
        }

        impl  GLPrimitive for $ty {})*
    };
}
macro_rules! impl_prim_for_vecs_impl {
    ($($ty:ident => <$prim:ident x $count:ident>;)*) => {
        $(
            impl sealed::SealedPrimitive for $ty {
                const COMPONENT: VertexComponent = VertexComponent {
                    count: ComponentCount::$count,
                    .. <$prim as sealed::SealedPrimitive>::COMPONENT
                };
            }
             impl  GLPrimitive for $ty {}
        )*
    };
}
macro_rules! impl_prim_for_vecs {
    ($($prim:ident => [$_2:ident, $_3:ident, $_4:ident];)*) => {
        impl_prim_for_vecs_impl!{$(
            $_2 => <$prim x _2>;
            $_3 => <$prim x _3>;
            $_4 => <$prim x _4>;
        )*}
    };
}

impl_prim_for_prim! {
    f32 => Float;
    // f64 => Double;
    u8 => UnsignedByte;
    i8 => Byte;
    u16 => UnsignedShort;
    i16 => Short;
    u32 => UnsignedInt;
    i32 => SignedInt;

}
impl_prim_for_vecs! {
    f32 => [Vec2, Vec3, Vec4];
    // f64 => [DVec2, DVec3, DVec4];
    u8 => [U8Vec2, U8Vec3, U8Vec4];
    i8 => [I8Vec2, I8Vec3, I8Vec4];
    u32 => [UVec2, UVec3, UVec4];
    i32 => [IVec2, IVec3, IVec4];
    u16 => [U16Vec2, U16Vec3, U16Vec4];
    i16 => [I16Vec2, I16Vec3, I16Vec4];

}

/// A wrapper around a GL primitive to make it normalized in the vertex shader
#[derive(Default, Clone, Copy, Pod, Zeroable, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Normalized<T>(pub T)
where
    T: GLPrimitive;

impl<T: GLPrimitive> Normalized<T> {
    pub const fn new(prim: T) -> Self {
        Self(prim)
    }
}

impl<T: GLPrimitive> SealedPrimitive for Normalized<T> {
    const COMPONENT: VertexComponent = VertexComponent {
        normalized: true,
        ..T::COMPONENT
    };
}
impl<T: GLPrimitive> GLPrimitive for Normalized<T> {}
