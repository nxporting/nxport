use super::context::CodeGenContext;
use crate::ir::storage_kind::IoVariable;
use crate::structured_ir::ast::AstArena;
use crate::structured_ir::program::{IoDefinition, StructuredFunction, StructuredProgramInfo};
use rspirv::spirv;

pub struct Declarations;

impl Declarations {
    pub fn declare_all(context: &mut CodeGenContext, info: &StructuredProgramInfo) {
        Self::declare_constant_buffers(context, info);
        Self::declare_inputs_and_outputs(context, info);
    }

    pub fn declare_locals(context: &mut CodeGenContext, _arena: &AstArena, function: &StructuredFunction) {
        for &local_id in &function.locals {
            let var_type = context.f32_type; // Default to float/dword size
            let ptr_type = context.builder.type_pointer(None, spirv::StorageClass::Function, var_type);
            let var_id = context.builder.variable(ptr_type, None, spirv::StorageClass::Function, None);
            context.locals.insert(local_id, var_id);
        }
    }

    fn declare_constant_buffers(context: &mut CodeGenContext, info: &StructuredProgramInfo) {
        let mut sorted_bindings: Vec<i32> = info.cbuf_bindings.iter().copied().collect();
        sorted_bindings.sort();

        let elem_type = context.v4f32_type;
        // Large array size for uniform buffer (e.g., 4096 vec4s)
        let array_size = context.const_u32(4096);
        let array_type = context.builder.type_array(elem_type, array_size);
        context.builder.decorate(
            array_type,
            spirv::Decoration::ArrayStride,
            vec![rspirv::dr::Operand::LiteralBit32(16)],
        );

        let struct_type = context.builder.type_struct(vec![array_type]);
        context.builder.decorate(struct_type, spirv::Decoration::Block, vec![]);
        context.builder.member_decorate(
            struct_type,
            0,
            spirv::Decoration::Offset,
            vec![rspirv::dr::Operand::LiteralBit32(0)],
        );

        let ptr_type = context.builder.type_pointer(None, spirv::StorageClass::Uniform, struct_type);

        for slot in sorted_bindings {
            let var_id = context.builder.variable(ptr_type, None, spirv::StorageClass::Uniform, None);
            context.builder.decorate(
                var_id,
                spirv::Decoration::DescriptorSet,
                vec![rspirv::dr::Operand::LiteralBit32(0)],
            );
            context.builder.decorate(
                var_id,
                spirv::Decoration::Binding,
                vec![rspirv::dr::Operand::LiteralBit32(slot as u32)],
            );
            context.constant_buffers.insert(slot, var_id);
        }
    }

    fn declare_inputs_and_outputs(context: &mut CodeGenContext, info: &StructuredProgramInfo) {
        let mut sorted_ios: Vec<IoDefinition> = info.io_definitions.iter().cloned().collect();
        sorted_ios.sort_by_key(|io| (io.storage_kind as u32, io.io_variable as u32, io.location, io.component));

        for io in sorted_ios {
            let is_output = io.storage_kind.is_output();
            let storage_class = if is_output {
                spirv::StorageClass::Output
            } else {
                spirv::StorageClass::Input
            };

            let var_type = context.v4f32_type;
            let ptr_type = context.builder.type_pointer(None, storage_class, var_type);
            let var_id = context.builder.variable(ptr_type, None, storage_class, None);

            match io.io_variable {
                IoVariable::Position => {
                    context.builder.decorate(
                        var_id,
                        spirv::Decoration::BuiltIn,
                        vec![rspirv::dr::Operand::BuiltIn(spirv::BuiltIn::Position)],
                    );
                }
                IoVariable::PointSize => {
                    context.builder.decorate(
                        var_id,
                        spirv::Decoration::BuiltIn,
                        vec![rspirv::dr::Operand::BuiltIn(spirv::BuiltIn::PointSize)],
                    );
                }
                IoVariable::ClipDistance => {
                    context.builder.decorate(
                        var_id,
                        spirv::Decoration::BuiltIn,
                        vec![rspirv::dr::Operand::BuiltIn(spirv::BuiltIn::ClipDistance)],
                    );
                }
                IoVariable::FragmentCoord => {
                    context.builder.decorate(
                        var_id,
                        spirv::Decoration::BuiltIn,
                        vec![rspirv::dr::Operand::BuiltIn(spirv::BuiltIn::FragCoord)],
                    );
                }
                IoVariable::FragmentOutputDepth => {
                    context.builder.decorate(
                        var_id,
                        spirv::Decoration::BuiltIn,
                        vec![rspirv::dr::Operand::BuiltIn(spirv::BuiltIn::FragDepth)],
                    );
                }
                IoVariable::FragmentOutputColor | IoVariable::UserDefined => {
                    context.builder.decorate(
                        var_id,
                        spirv::Decoration::Location,
                        vec![rspirv::dr::Operand::LiteralBit32(io.location as u32)],
                    );
                }
                _ => {
                    context.builder.decorate(
                        var_id,
                        spirv::Decoration::Location,
                        vec![rspirv::dr::Operand::LiteralBit32(io.location as u32)],
                    );
                }
            }

            context.main_interface.push(var_id);
            if is_output {
                context.outputs.insert(io, var_id);
            } else {
                context.inputs.insert(io, var_id);
            }
        }
    }
}
