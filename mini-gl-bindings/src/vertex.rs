use std::{alloc::Layout, fmt::Display};

use bytemuck::Pod;

use crate::{GLPrimitive, primitive::DataType};

/// The number of components in a [`VertexComponent`]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u32)]
pub enum ComponentCount {
    _1 = 1,
    _2 = 2,
    _3 = 3,
    _4 = 4,
}
/// A component of a [`Vertex`]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VertexComponent {
    pub ty: DataType,
    pub count: ComponentCount,
    pub normalized: bool,
}

impl VertexComponent {
    const fn component_size(self) -> usize {
        match self.ty {
            // DataType::Double => size_of::<f64>(),
            DataType::Float => size_of::<f32>(),
            DataType::SignedInt => size_of::<i32>(),
            DataType::UnsignedInt => size_of::<u32>(),
            DataType::Byte => size_of::<i8>(),
            DataType::UnsignedByte => size_of::<u8>(),
            DataType::Short => size_of::<u16>(),
            DataType::UnsignedShort => size_of::<u16>(),
        }
    }
    /// The total size of the component, as if done with [`size_of`]
    pub const fn size(self) -> usize {
        self.component_size() * (self.count as usize)
    }
    /// The minimum alignment of the type as required by OpenGL
    pub fn align(self) -> usize {
        match self.count {
            n @ (ComponentCount::_1 | ComponentCount::_2) => self.component_size() * n as usize,
            ComponentCount::_3 | ComponentCount::_4 => self.component_size() * 4,
        }
    }
    /// The size and alignment of the [`VertexComponent`] as a [`Layout`]
    pub fn layout(self) -> Layout {
        Layout::from_size_align(self.size(), self.align()).unwrap()
    }
}

#[doc(hidden)]
pub const fn component_of<T: GLPrimitive>(_: &T) -> VertexComponent {
    component_of_prim_type::<T>()
}
pub(crate) const fn component_of_prim_type<T: GLPrimitive>() -> VertexComponent {
    T::COMPONENT
}
#[macro_export]
macro_rules! impl_vertex {
    ($ty:ty { $($fields:ident),* }) => {
        unsafe impl $crate::Vertex for $ty {
            const FIELDS: &[($crate::VertexComponent, ::core::primitive::usize)] = {
                /// SAFETY: Vertex implies Zeroable
                let dummy = unsafe { ::core::mem::MaybeUninit::<Self>::zeroed().assume_init() };
                &[$(($crate::component_of(&dummy.$fields), ::core::mem::offset_of!(Self, $fields))),*]
            };
        }
    };
}

/// A type representing Vertex data.
///
/// # Safety
/// This should never be implemented outside of the [`impl_vertex`] macro.
/// The internal details are destined to change.
pub unsafe trait Vertex: bytemuck::Pod {
    #[doc(hidden)]
    const FIELDS: &[(VertexComponent, usize)];
}

#[derive(Debug)]
pub enum VertexBuildError {
    ComponentsDontMatch,
    NotEnoughData,
}
impl Display for VertexBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ComponentsDontMatch => {
                write!(f, "Components don't match the layout previously specified")
            }
            Self::NotEnoughData => write!(f, "Not enough data to make a vertex buffer"),
        }
    }
}
pub struct VertexBuilder {
    components: Vec<(VertexComponent, usize)>,
    bytes: Vec<u8>,
    vertex_size: i32,
    current_offset: usize,
    current_field: usize,
}
impl VertexBuilder {
    pub fn new(components: impl IntoIterator<Item = VertexComponent> + Clone) -> Self {
        let mut layout = Layout::from_size_align(0, 1).unwrap();
        let components = components
            .into_iter()
            .scan(&mut layout, |layout, component| {
                let (new_layout, offset) = layout.extend(component.layout()).unwrap();
                **layout = new_layout;
                Some((component, offset))
            });
        Self {
            components: components.collect(),
            bytes: vec![],
            vertex_size: layout.size().try_into().expect("Layout is too large lol"),
            current_field: 0,
            current_offset: 0,
        }
    }
    pub fn field<T: Pod + GLPrimitive>(mut self, data: &T) -> Result<Self, VertexBuildError> {
        if self.current_field >= self.components.len() {
            self.current_offset = 0;
            self.current_field = 0;
        }
        let (component, offset) = self.components[self.current_field];
        let layout = component.layout();
        if Layout::new::<T>().size() != component.size() {
            return Err(VertexBuildError::ComponentsDontMatch);
        }
        if offset > self.current_offset {
            let dif = offset - self.current_offset;
            self.bytes.resize(self.bytes.len() + dif, 0);
        }
        self.bytes.extend_from_slice(bytemuck::bytes_of(data));
        self.current_offset = offset + layout.size();
        self.current_field += 1;
        Ok(self)
    }
    #[allow(clippy::type_complexity, reason = "stop")]
    pub fn build(self) -> Result<VertexData, VertexBuildError> {
        if self.current_field < self.components.len() {
            return Err(VertexBuildError::NotEnoughData);
        }
        Ok(VertexData {
            bytes: self.bytes,
            components: self.components,
            vertex_size: self.vertex_size,
        })
    }
}

/// Valid vertex data that can be converted into a vertex buffer. Contains the fields and bytes of a vertex.
// Not used yet. Will be used once I support custom vertex formats for Hypatia.
pub struct VertexData {
    pub(crate) bytes: Vec<u8>,
    pub(crate) components: Vec<(VertexComponent, usize)>,
    pub(crate) vertex_size: i32,
}

#[cfg(test)]
mod test {
    use glam::{UVec3, Vec2, Vec3};

    use crate::{VertexBuildError, VertexBuilder, component_of_prim_type};
    #[test]
    fn vertex_builder_basic() {
        let data = VertexBuilder::new([
            component_of_prim_type::<Vec3>(),
            component_of_prim_type::<f32>(),
            component_of_prim_type::<Vec2>(),
        ])
        .field(&Vec3::new(0.0, 1.0, 2.0))
        .unwrap()
        .field(&9.9)
        .unwrap()
        .field(&Vec2::new(1.0, 1.0))
        .unwrap()
        .build()
        .unwrap();

        assert_eq!(
            data.bytes,
            [
                0, 0, 0, 0, 0, 0, 128, 63, 0, 0, 0, 64, 102, 102, 30, 65, 0, 0, 128, 63, 0, 0, 128,
                63
            ]
        );
    }

    #[test]
    fn vertex_builder_pads_properly() {
        let data = VertexBuilder::new([
            component_of_prim_type::<f32>(),
            component_of_prim_type::<Vec3>(),
        ])
        .field(&1.0)
        .unwrap()
        .field(&Vec3::new(1.0, 1.0, 1.0))
        .unwrap()
        .build()
        .unwrap();
        assert_eq!(
            data.bytes,
            [
                0, 0, 128, 63, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 128, 63, 0, 0, 128, 63, 0,
                0, 128, 63
            ]
        );
        assert_eq!(
            data.components,
            vec![
                (component_of_prim_type::<f32>(), 0),
                (component_of_prim_type::<Vec3>(), 16)
            ]
        )
    }
    #[test]
    fn vertex_builder_repeat() {
        let data = VertexBuilder::new([
            component_of_prim_type::<f32>(),
            component_of_prim_type::<Vec3>(),
        ])
        .field(&1.0)
        .unwrap()
        .field(&Vec3::new(1.0, 1.0, 1.0))
        .unwrap()
        .field(&1.0)
        .unwrap()
        .field(&Vec3::new(1.0, 0.5, 1.0))
        .unwrap()
        .build()
        .unwrap();
        assert_eq!(
            data.bytes,
            [
                0, 0, 128, 63, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 128, 63, 0, 0, 128, 63, 0,
                0, 128, 63, 0, 0, 128, 63, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 128, 63, 0, 0,
                0, 63, 0, 0, 128, 63
            ]
        );
    }
    #[test]
    fn vertex_builder_not_enough_data() {
        let res = VertexBuilder::new([
            component_of_prim_type::<f32>(),
            component_of_prim_type::<Vec3>(),
        ])
        .field(&1.0)
        .unwrap()
        .build();
        match res {
            Ok(_) | Err(VertexBuildError::ComponentsDontMatch) => {
                panic!("Vertex builder should not have enough data")
            }
            Err(VertexBuildError::NotEnoughData) => (),
        }
    }
    #[test]
    fn vertex_builder_repeat_not_enough_data() {
        let res = VertexBuilder::new([
            component_of_prim_type::<f32>(),
            component_of_prim_type::<Vec3>(),
        ])
        .field(&1.0)
        .unwrap()
        .field(&Vec3::new(1.0, 1.0, 1.0))
        .unwrap()
        .field(&0.1)
        .unwrap()
        .build();
        match res {
            Ok(_) | Err(VertexBuildError::ComponentsDontMatch) => {
                panic!("Vertex builder should not have enough data")
            }
            Err(VertexBuildError::NotEnoughData) => (),
        }
    }
    #[test]
    fn vertex_builder_repeat_not_enough_data_2() {
        let res = VertexBuilder::new([
            component_of_prim_type::<f32>(),
            component_of_prim_type::<Vec3>(),
        ])
        .field(&1.0)
        .unwrap()
        .field(&Vec3::new(1.0, 1.0, 1.0))
        .unwrap()
        .field(&1.0)
        .unwrap()
        .field(&Vec3::new(1.0, 0.5, 1.0))
        .unwrap()
        .field(&0.1)
        .unwrap()
        .build();
        match res {
            Ok(_) | Err(VertexBuildError::ComponentsDontMatch) => {
                panic!("Vertex builder should not have enough data")
            }
            Err(VertexBuildError::NotEnoughData) => (),
        }
    }
    #[test]
    fn vertex_builder_invalid_field() {
        let res = VertexBuilder::new([
            component_of_prim_type::<f32>(),
            component_of_prim_type::<Vec3>(),
        ])
        .field(&1.0)
        .unwrap()
        .field(&1.0);
        match res {
            Ok(_) | Err(VertexBuildError::NotEnoughData) => {
                panic!("Vertex builder should have invalid components")
            }
            Err(VertexBuildError::ComponentsDontMatch) => (),
        }
    }
    #[test]
    fn vertex_builder_weird() {
        let data = VertexBuilder::new([
            component_of_prim_type::<f32>(),
            component_of_prim_type::<Vec3>(),
        ])
        .field(&10000u32)
        .unwrap()
        .field(&UVec3::new(100, 2, 0))
        .unwrap()
        .field(&1.0)
        .unwrap()
        .field(&Vec3::new(1.0, 0.5, 1.0))
        .unwrap()
        .build()
        .unwrap();
        assert_eq!(
            data.bytes,
            [
                16, 39, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 100, 0, 0, 0, 2, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 128, 63, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 128, 63, 0, 0, 0,
                63, 0, 0, 128, 63
            ]
        );
    }
}
