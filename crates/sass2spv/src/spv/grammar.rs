// Generated from Spv.Generator's CoreGrammar.cs and GlslStd450Grammar.cs (work/tools/re/gen_spv_grammar.py).
// One method per instruction: the operands in order, the instruction in its section.

#![allow(clippy::too_many_arguments, clippy::all)]

use super::*;

#[rustfmt::skip]
impl Module {
    pub fn nop(&mut self, ) -> Inst {
        let result = self.new_instruction(Op::NOP, INVALID_ID, None);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn undef(&mut self, result_type: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::UNDEF, result_id, Some(result_type));
        self.add_to_function_definitions(&result);
        result
    }

    pub fn size_of(&mut self, result_type: &Inst, pointer: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::SIZE_OF, result_id, Some(result_type));
        result.add_operand(pointer);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn source_continued(&mut self, continued_source: &str) -> Inst {
        let result = self.new_instruction(Op::SOURCE_CONTINUED, INVALID_ID, None);
        result.add_operand(continued_source);
        self.add_debug(&result);
        result
    }

    pub fn source(&mut self, source_language: SourceLanguage, version: Lit, file: Option<&Inst>, source: Option<&str>) -> Inst {
        let result = self.new_instruction(Op::SOURCE, INVALID_ID, None);
        result.add_operand(source_language);
        result.add_operand(version);
        if let Some(v) = file {
            result.add_operand(v);
        }
        if let Some(v) = source {
            result.add_operand(v);
        }
        self.add_debug(&result);
        result
    }

    pub fn source_extension(&mut self, extension: &str) -> Inst {
        let result = self.new_instruction(Op::SOURCE_EXTENSION, INVALID_ID, None);
        result.add_operand(extension);
        self.add_debug(&result);
        result
    }

    pub fn name(&mut self, target: &Inst, name: &str) -> Inst {
        let result = self.new_instruction(Op::NAME, INVALID_ID, None);
        result.add_operand(target);
        result.add_operand(name);
        self.add_debug(&result);
        result
    }

    pub fn member_name(&mut self, type_: &Inst, member: Lit, name: &str) -> Inst {
        let result = self.new_instruction(Op::MEMBER_NAME, INVALID_ID, None);
        result.add_operand(type_);
        result.add_operand(member);
        result.add_operand(name);
        self.add_debug(&result);
        result
    }

    pub fn string(&mut self, str: &str) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::STRING, result_id, None);
        result.add_operand(str);
        self.add_debug(&result);
        result
    }

    pub fn line(&mut self, file: &Inst, line: Lit, column: Lit) -> Inst {
        let result = self.new_instruction(Op::LINE, INVALID_ID, None);
        result.add_operand(file);
        result.add_operand(line);
        result.add_operand(column);
        self.add_debug(&result);
        result
    }

    pub fn no_line(&mut self, ) -> Inst {
        let result = self.new_instruction(Op::NO_LINE, INVALID_ID, None);
        self.add_debug(&result);
        result
    }

    pub fn module_processed(&mut self, process: &str) -> Inst {
        let result = self.new_instruction(Op::MODULE_PROCESSED, INVALID_ID, None);
        result.add_operand(process);
        self.add_debug(&result);
        result
    }

    pub fn decorate(&mut self, target: &Inst, decoration: Decoration, parameters: &[Operand]) -> Inst {
        let result = self.new_instruction(Op::DECORATE, INVALID_ID, None);
        result.add_operand(target);
        result.add_operand(decoration);
        result.add_operands(parameters);
        self.add_annotation(&result);
        result
    }

    pub fn member_decorate(&mut self, structure_type: &Inst, member: Lit, decoration: Decoration, parameters: &[Operand]) -> Inst {
        let result = self.new_instruction(Op::MEMBER_DECORATE, INVALID_ID, None);
        result.add_operand(structure_type);
        result.add_operand(member);
        result.add_operand(decoration);
        result.add_operands(parameters);
        self.add_annotation(&result);
        result
    }

    pub fn decoration_group(&mut self, ) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::DECORATION_GROUP, result_id, None);
        self.add_annotation(&result);
        result
    }

    pub fn group_decorate(&mut self, decoration_group: &Inst, targets: &[Inst]) -> Inst {
        let result = self.new_instruction(Op::GROUP_DECORATE, INVALID_ID, None);
        result.add_operand(decoration_group);
        result.add_operands(targets);
        self.add_annotation(&result);
        result
    }

    pub fn group_member_decorate(&mut self, decoration_group: &Inst, targets: &[Operand]) -> Inst {
        let result = self.new_instruction(Op::GROUP_MEMBER_DECORATE, INVALID_ID, None);
        result.add_operand(decoration_group);
        result.add_operands(targets);
        self.add_annotation(&result);
        result
    }

    pub fn decorate_id(&mut self, target: &Inst, decoration: Decoration, parameters: &[Operand]) -> Inst {
        let result = self.new_instruction(Op::DECORATE_ID, INVALID_ID, None);
        result.add_operand(target);
        result.add_operand(decoration);
        result.add_operands(parameters);
        self.add_annotation(&result);
        result
    }

    pub fn decorate_string(&mut self, target: &Inst, decoration: Decoration, parameters: &[Operand]) -> Inst {
        let result = self.new_instruction(Op::DECORATE_STRING, INVALID_ID, None);
        result.add_operand(target);
        result.add_operand(decoration);
        result.add_operands(parameters);
        self.add_annotation(&result);
        result
    }

    pub fn decorate_string_google(&mut self, target: &Inst, decoration: Decoration, parameters: &[Operand]) -> Inst {
        let result = self.new_instruction(Op::DECORATE_STRING, INVALID_ID, None);
        result.add_operand(target);
        result.add_operand(decoration);
        result.add_operands(parameters);
        self.add_annotation(&result);
        result
    }

    pub fn member_decorate_string(&mut self, struct_type: &Inst, member: Lit, decoration: Decoration, parameters: &[Operand]) -> Inst {
        let result = self.new_instruction(Op::MEMBER_DECORATE_STRING, INVALID_ID, None);
        result.add_operand(struct_type);
        result.add_operand(member);
        result.add_operand(decoration);
        result.add_operands(parameters);
        self.add_annotation(&result);
        result
    }

    pub fn member_decorate_string_google(&mut self, struct_type: &Inst, member: Lit, decoration: Decoration, parameters: &[Operand]) -> Inst {
        let result = self.new_instruction(Op::MEMBER_DECORATE_STRING, INVALID_ID, None);
        result.add_operand(struct_type);
        result.add_operand(member);
        result.add_operand(decoration);
        result.add_operands(parameters);
        self.add_annotation(&result);
        result
    }

    pub fn type_void(&mut self, force_id_allocation: bool) -> Inst {
        let result = self.new_instruction(Op::TYPE_VOID, INVALID_ID, None);
        self.add_type_declaration(&result, force_id_allocation);
        result
    }

    pub fn type_bool(&mut self, force_id_allocation: bool) -> Inst {
        let result = self.new_instruction(Op::TYPE_BOOL, INVALID_ID, None);
        self.add_type_declaration(&result, force_id_allocation);
        result
    }

    pub fn type_int(&mut self, width: Lit, signedness: Lit, force_id_allocation: bool) -> Inst {
        let result = self.new_instruction(Op::TYPE_INT, INVALID_ID, None);
        result.add_operand(width);
        result.add_operand(signedness);
        self.add_type_declaration(&result, force_id_allocation);
        result
    }

    pub fn type_float(&mut self, width: Lit, force_id_allocation: bool) -> Inst {
        let result = self.new_instruction(Op::TYPE_FLOAT, INVALID_ID, None);
        result.add_operand(width);
        self.add_type_declaration(&result, force_id_allocation);
        result
    }

    pub fn type_vector(&mut self, component_type: &Inst, component_count: Lit, force_id_allocation: bool) -> Inst {
        let result = self.new_instruction(Op::TYPE_VECTOR, INVALID_ID, None);
        result.add_operand(component_type);
        result.add_operand(component_count);
        self.add_type_declaration(&result, force_id_allocation);
        result
    }

    pub fn type_matrix(&mut self, column_type: &Inst, column_count: Lit, force_id_allocation: bool) -> Inst {
        let result = self.new_instruction(Op::TYPE_MATRIX, INVALID_ID, None);
        result.add_operand(column_type);
        result.add_operand(column_count);
        self.add_type_declaration(&result, force_id_allocation);
        result
    }

    pub fn type_image(&mut self, sampled_type: &Inst, dim: Dim, depth: Lit, arrayed: Lit, m_s: Lit, sampled: Lit, image_format: ImageFormat, access_qualifier: Option<AccessQualifier>, force_id_allocation: bool) -> Inst {
        let result = self.new_instruction(Op::TYPE_IMAGE, INVALID_ID, None);
        result.add_operand(sampled_type);
        result.add_operand(dim);
        result.add_operand(depth);
        result.add_operand(arrayed);
        result.add_operand(m_s);
        result.add_operand(sampled);
        result.add_operand(image_format);
        if let Some(v) = access_qualifier {
            result.add_operand(v);
        }
        self.add_type_declaration(&result, force_id_allocation);
        result
    }

    pub fn type_sampler(&mut self, force_id_allocation: bool) -> Inst {
        let result = self.new_instruction(Op::TYPE_SAMPLER, INVALID_ID, None);
        self.add_type_declaration(&result, force_id_allocation);
        result
    }

    pub fn type_sampled_image(&mut self, image_type: &Inst, force_id_allocation: bool) -> Inst {
        let result = self.new_instruction(Op::TYPE_SAMPLED_IMAGE, INVALID_ID, None);
        result.add_operand(image_type);
        self.add_type_declaration(&result, force_id_allocation);
        result
    }

    pub fn type_array(&mut self, element_type: &Inst, length: &Inst, force_id_allocation: bool) -> Inst {
        let result = self.new_instruction(Op::TYPE_ARRAY, INVALID_ID, None);
        result.add_operand(element_type);
        result.add_operand(length);
        self.add_type_declaration(&result, force_id_allocation);
        result
    }

    pub fn type_runtime_array(&mut self, element_type: &Inst, force_id_allocation: bool) -> Inst {
        let result = self.new_instruction(Op::TYPE_RUNTIME_ARRAY, INVALID_ID, None);
        result.add_operand(element_type);
        self.add_type_declaration(&result, force_id_allocation);
        result
    }

    pub fn type_struct(&mut self, force_id_allocation: bool, parameters: &[Inst]) -> Inst {
        let result = self.new_instruction(Op::TYPE_STRUCT, INVALID_ID, None);
        result.add_operands(parameters);
        self.add_type_declaration(&result, force_id_allocation);
        result
    }

    pub fn type_opaque(&mut self, thenameoftheopaquetype: &str, force_id_allocation: bool) -> Inst {
        let result = self.new_instruction(Op::TYPE_OPAQUE, INVALID_ID, None);
        result.add_operand(thenameoftheopaquetype);
        self.add_type_declaration(&result, force_id_allocation);
        result
    }

    pub fn type_pointer(&mut self, storage_class: StorageClass, type_: &Inst, force_id_allocation: bool) -> Inst {
        let result = self.new_instruction(Op::TYPE_POINTER, INVALID_ID, None);
        result.add_operand(storage_class);
        result.add_operand(type_);
        self.add_type_declaration(&result, force_id_allocation);
        result
    }

    pub fn type_function(&mut self, return_type: &Inst, force_id_allocation: bool, parameters: &[Inst]) -> Inst {
        let result = self.new_instruction(Op::TYPE_FUNCTION, INVALID_ID, None);
        result.add_operand(return_type);
        result.add_operands(parameters);
        self.add_type_declaration(&result, force_id_allocation);
        result
    }

    pub fn type_event(&mut self, force_id_allocation: bool) -> Inst {
        let result = self.new_instruction(Op::TYPE_EVENT, INVALID_ID, None);
        self.add_type_declaration(&result, force_id_allocation);
        result
    }

    pub fn type_device_event(&mut self, force_id_allocation: bool) -> Inst {
        let result = self.new_instruction(Op::TYPE_DEVICE_EVENT, INVALID_ID, None);
        self.add_type_declaration(&result, force_id_allocation);
        result
    }

    pub fn type_reserve_id(&mut self, force_id_allocation: bool) -> Inst {
        let result = self.new_instruction(Op::TYPE_RESERVE_ID, INVALID_ID, None);
        self.add_type_declaration(&result, force_id_allocation);
        result
    }

    pub fn type_queue(&mut self, force_id_allocation: bool) -> Inst {
        let result = self.new_instruction(Op::TYPE_QUEUE, INVALID_ID, None);
        self.add_type_declaration(&result, force_id_allocation);
        result
    }

    pub fn type_pipe(&mut self, qualifier: AccessQualifier, force_id_allocation: bool) -> Inst {
        let result = self.new_instruction(Op::TYPE_PIPE, INVALID_ID, None);
        result.add_operand(qualifier);
        self.add_type_declaration(&result, force_id_allocation);
        result
    }

    pub fn type_forward_pointer(&mut self, pointer_type: &Inst, storage_class: StorageClass, force_id_allocation: bool) -> Inst {
        let result = self.new_instruction(Op::TYPE_FORWARD_POINTER, INVALID_ID, None);
        result.add_operand(pointer_type);
        result.add_operand(storage_class);
        self.add_type_declaration(&result, force_id_allocation);
        result
    }

    pub fn type_pipe_storage(&mut self, force_id_allocation: bool) -> Inst {
        let result = self.new_instruction(Op::TYPE_PIPE_STORAGE, INVALID_ID, None);
        self.add_type_declaration(&result, force_id_allocation);
        result
    }

    pub fn type_named_barrier(&mut self, force_id_allocation: bool) -> Inst {
        let result = self.new_instruction(Op::TYPE_NAMED_BARRIER, INVALID_ID, None);
        self.add_type_declaration(&result, force_id_allocation);
        result
    }

    pub fn constant_true(&mut self, result_type: &Inst) -> Inst {
        let result = self.new_instruction(Op::CONSTANT_TRUE, INVALID_ID, Some(result_type));
        self.add_constant(&result);
        result
    }

    pub fn constant_false(&mut self, result_type: &Inst) -> Inst {
        let result = self.new_instruction(Op::CONSTANT_FALSE, INVALID_ID, Some(result_type));
        self.add_constant(&result);
        result
    }

    pub fn constant(&mut self, result_type: &Inst, value: Lit) -> Inst {
        let result = self.new_instruction(Op::CONSTANT, INVALID_ID, Some(result_type));
        result.add_operand(value);
        self.add_constant(&result);
        result
    }

    pub fn constant_composite(&mut self, result_type: &Inst, constituents: &[Inst]) -> Inst {
        let result = self.new_instruction(Op::CONSTANT_COMPOSITE, INVALID_ID, Some(result_type));
        result.add_operands(constituents);
        self.add_constant(&result);
        result
    }

    pub fn constant_sampler(&mut self, result_type: &Inst, sampler_addressing_mode: SamplerAddressingMode, param: Lit, sampler_filter_mode: SamplerFilterMode) -> Inst {
        let result = self.new_instruction(Op::CONSTANT_SAMPLER, INVALID_ID, Some(result_type));
        result.add_operand(sampler_addressing_mode);
        result.add_operand(param);
        result.add_operand(sampler_filter_mode);
        self.add_constant(&result);
        result
    }

    pub fn constant_null(&mut self, result_type: &Inst) -> Inst {
        let result = self.new_instruction(Op::CONSTANT_NULL, INVALID_ID, Some(result_type));
        self.add_constant(&result);
        result
    }

    pub fn spec_constant_true(&mut self, result_type: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::SPEC_CONSTANT_TRUE, result_id, Some(result_type));
        self.add_to_function_definitions(&result);
        result
    }

    pub fn spec_constant_false(&mut self, result_type: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::SPEC_CONSTANT_FALSE, result_id, Some(result_type));
        self.add_to_function_definitions(&result);
        result
    }

    pub fn spec_constant(&mut self, result_type: &Inst, value: Lit) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::SPEC_CONSTANT, result_id, Some(result_type));
        result.add_operand(value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn spec_constant_composite(&mut self, result_type: &Inst, constituents: &[Inst]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::SPEC_CONSTANT_COMPOSITE, result_id, Some(result_type));
        result.add_operands(constituents);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn spec_constant_op(&mut self, result_type: &Inst, opcode: Lit) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::SPEC_CONSTANT_OP, result_id, Some(result_type));
        result.add_operand(opcode);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn variable(&mut self, result_type: &Inst, storage_class: StorageClass, initializer: Option<&Inst>) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::VARIABLE, result_id, Some(result_type));
        result.add_operand(storage_class);
        if let Some(v) = initializer {
            result.add_operand(v);
        }
        result
    }

    pub fn image_texel_pointer(&mut self, result_type: &Inst, image: &Inst, coordinate: &Inst, sample: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE_TEXEL_POINTER, result_id, Some(result_type));
        result.add_operand(image);
        result.add_operand(coordinate);
        result.add_operand(sample);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn load(&mut self, result_type: &Inst, pointer: &Inst, memory_access: Option<MemoryAccessMask>) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::LOAD, result_id, Some(result_type));
        result.add_operand(pointer);
        if let Some(v) = memory_access {
            result.add_operand(v);
        }
        self.add_to_function_definitions(&result);
        result
    }

    pub fn store(&mut self, pointer: &Inst, obj: &Inst, memory_access: Option<MemoryAccessMask>) -> Inst {
        let result = self.new_instruction(Op::STORE, INVALID_ID, None);
        result.add_operand(pointer);
        result.add_operand(obj);
        if let Some(v) = memory_access {
            result.add_operand(v);
        }
        self.add_to_function_definitions(&result);
        result
    }

    pub fn copy_memory(&mut self, target: &Inst, source: &Inst, memory_access0: Option<MemoryAccessMask>, memory_access1: Option<MemoryAccessMask>) -> Inst {
        let result = self.new_instruction(Op::COPY_MEMORY, INVALID_ID, None);
        result.add_operand(target);
        result.add_operand(source);
        if let Some(v) = memory_access0 {
            result.add_operand(v);
        }
        if let Some(v) = memory_access1 {
            result.add_operand(v);
        }
        self.add_to_function_definitions(&result);
        result
    }

    pub fn copy_memory_sized(&mut self, target: &Inst, source: &Inst, size: &Inst, memory_access0: Option<MemoryAccessMask>, memory_access1: Option<MemoryAccessMask>) -> Inst {
        let result = self.new_instruction(Op::COPY_MEMORY_SIZED, INVALID_ID, None);
        result.add_operand(target);
        result.add_operand(source);
        result.add_operand(size);
        if let Some(v) = memory_access0 {
            result.add_operand(v);
        }
        if let Some(v) = memory_access1 {
            result.add_operand(v);
        }
        self.add_to_function_definitions(&result);
        result
    }

    pub fn access_chain(&mut self, result_type: &Inst, base_obj: &Inst, indexes: &[Inst]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::ACCESS_CHAIN, result_id, Some(result_type));
        result.add_operand(base_obj);
        result.add_operands(indexes);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn in_bounds_access_chain(&mut self, result_type: &Inst, base_obj: &Inst, indexes: &[Inst]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IN_BOUNDS_ACCESS_CHAIN, result_id, Some(result_type));
        result.add_operand(base_obj);
        result.add_operands(indexes);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn ptr_access_chain(&mut self, result_type: &Inst, base_obj: &Inst, element: &Inst, indexes: &[Inst]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::PTR_ACCESS_CHAIN, result_id, Some(result_type));
        result.add_operand(base_obj);
        result.add_operand(element);
        result.add_operands(indexes);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn array_length(&mut self, result_type: &Inst, structure: &Inst, arraymember: Lit) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::ARRAY_LENGTH, result_id, Some(result_type));
        result.add_operand(structure);
        result.add_operand(arraymember);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn generic_ptr_mem_semantics(&mut self, result_type: &Inst, pointer: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GENERIC_PTR_MEM_SEMANTICS, result_id, Some(result_type));
        result.add_operand(pointer);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn in_bounds_ptr_access_chain(&mut self, result_type: &Inst, base_obj: &Inst, element: &Inst, indexes: &[Inst]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IN_BOUNDS_PTR_ACCESS_CHAIN, result_id, Some(result_type));
        result.add_operand(base_obj);
        result.add_operand(element);
        result.add_operands(indexes);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn ptr_equal(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::PTR_EQUAL, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn ptr_not_equal(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::PTR_NOT_EQUAL, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn ptr_diff(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::PTR_DIFF, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn function(&mut self, result_type: &Inst, function_control: FunctionControlMask, function_type: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::FUNCTION, result_id, Some(result_type));
        result.add_operand(function_control);
        result.add_operand(function_type);
        result
    }

    pub fn function_parameter(&mut self, result_type: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::FUNCTION_PARAMETER, result_id, Some(result_type));
        self.add_to_function_definitions(&result);
        result
    }

    pub fn function_end(&mut self, ) -> Inst {
        let result = self.new_instruction(Op::FUNCTION_END, INVALID_ID, None);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn function_call(&mut self, result_type: &Inst, function: &Inst, parameters: &[Inst]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::FUNCTION_CALL, result_id, Some(result_type));
        result.add_operand(function);
        result.add_operands(parameters);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn sampled_image(&mut self, result_type: &Inst, image: &Inst, sampler: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::SAMPLED_IMAGE, result_id, Some(result_type));
        result.add_operand(image);
        result.add_operand(sampler);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image_sample_implicit_lod(&mut self, result_type: &Inst, sampled_image: &Inst, coordinate: &Inst, image_operands: ImageOperandsMask, image_operand_ids: &[Inst]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE_SAMPLE_IMPLICIT_LOD, result_id, Some(result_type));
        result.add_operand(sampled_image);
        result.add_operand(coordinate);
        result.add_operand(image_operands);
        result.add_operands(image_operand_ids);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image_sample_explicit_lod(&mut self, result_type: &Inst, sampled_image: &Inst, coordinate: &Inst, image_operands: ImageOperandsMask, image_operand_ids: &[Inst]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE_SAMPLE_EXPLICIT_LOD, result_id, Some(result_type));
        result.add_operand(sampled_image);
        result.add_operand(coordinate);
        result.add_operand(image_operands);
        result.add_operands(image_operand_ids);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image_sample_dref_implicit_lod(&mut self, result_type: &Inst, sampled_image: &Inst, coordinate: &Inst, d_ref: &Inst, image_operands: ImageOperandsMask, image_operand_ids: &[Inst]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE_SAMPLE_DREF_IMPLICIT_LOD, result_id, Some(result_type));
        result.add_operand(sampled_image);
        result.add_operand(coordinate);
        result.add_operand(d_ref);
        result.add_operand(image_operands);
        result.add_operands(image_operand_ids);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image_sample_dref_explicit_lod(&mut self, result_type: &Inst, sampled_image: &Inst, coordinate: &Inst, d_ref: &Inst, image_operands: ImageOperandsMask, image_operand_ids: &[Inst]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE_SAMPLE_DREF_EXPLICIT_LOD, result_id, Some(result_type));
        result.add_operand(sampled_image);
        result.add_operand(coordinate);
        result.add_operand(d_ref);
        result.add_operand(image_operands);
        result.add_operands(image_operand_ids);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image_sample_proj_implicit_lod(&mut self, result_type: &Inst, sampled_image: &Inst, coordinate: &Inst, image_operands: ImageOperandsMask, image_operand_ids: &[Inst]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE_SAMPLE_PROJ_IMPLICIT_LOD, result_id, Some(result_type));
        result.add_operand(sampled_image);
        result.add_operand(coordinate);
        result.add_operand(image_operands);
        result.add_operands(image_operand_ids);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image_sample_proj_explicit_lod(&mut self, result_type: &Inst, sampled_image: &Inst, coordinate: &Inst, image_operands: ImageOperandsMask, image_operand_ids: &[Inst]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE_SAMPLE_PROJ_EXPLICIT_LOD, result_id, Some(result_type));
        result.add_operand(sampled_image);
        result.add_operand(coordinate);
        result.add_operand(image_operands);
        result.add_operands(image_operand_ids);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image_sample_proj_dref_implicit_lod(&mut self, result_type: &Inst, sampled_image: &Inst, coordinate: &Inst, d_ref: &Inst, image_operands: ImageOperandsMask, image_operand_ids: &[Inst]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE_SAMPLE_PROJ_DREF_IMPLICIT_LOD, result_id, Some(result_type));
        result.add_operand(sampled_image);
        result.add_operand(coordinate);
        result.add_operand(d_ref);
        result.add_operand(image_operands);
        result.add_operands(image_operand_ids);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image_sample_proj_dref_explicit_lod(&mut self, result_type: &Inst, sampled_image: &Inst, coordinate: &Inst, d_ref: &Inst, image_operands: ImageOperandsMask, image_operand_ids: &[Inst]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE_SAMPLE_PROJ_DREF_EXPLICIT_LOD, result_id, Some(result_type));
        result.add_operand(sampled_image);
        result.add_operand(coordinate);
        result.add_operand(d_ref);
        result.add_operand(image_operands);
        result.add_operands(image_operand_ids);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image_fetch(&mut self, result_type: &Inst, image: &Inst, coordinate: &Inst, image_operands: ImageOperandsMask, image_operand_ids: &[Inst]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE_FETCH, result_id, Some(result_type));
        result.add_operand(image);
        result.add_operand(coordinate);
        result.add_operand(image_operands);
        result.add_operands(image_operand_ids);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image_gather(&mut self, result_type: &Inst, sampled_image: &Inst, coordinate: &Inst, component: &Inst, image_operands: ImageOperandsMask, image_operand_ids: &[Inst]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE_GATHER, result_id, Some(result_type));
        result.add_operand(sampled_image);
        result.add_operand(coordinate);
        result.add_operand(component);
        result.add_operand(image_operands);
        result.add_operands(image_operand_ids);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image_dref_gather(&mut self, result_type: &Inst, sampled_image: &Inst, coordinate: &Inst, d_ref: &Inst, image_operands: ImageOperandsMask, image_operand_ids: &[Inst]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE_DREF_GATHER, result_id, Some(result_type));
        result.add_operand(sampled_image);
        result.add_operand(coordinate);
        result.add_operand(d_ref);
        result.add_operand(image_operands);
        result.add_operands(image_operand_ids);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image_read(&mut self, result_type: &Inst, image: &Inst, coordinate: &Inst, image_operands: ImageOperandsMask, image_operand_ids: &[Inst]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE_READ, result_id, Some(result_type));
        result.add_operand(image);
        result.add_operand(coordinate);
        result.add_operand(image_operands);
        result.add_operands(image_operand_ids);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image_write(&mut self, image: &Inst, coordinate: &Inst, texel: &Inst, image_operands: ImageOperandsMask, image_operand_ids: &[Inst]) -> Inst {
        let result = self.new_instruction(Op::IMAGE_WRITE, INVALID_ID, None);
        result.add_operand(image);
        result.add_operand(coordinate);
        result.add_operand(texel);
        result.add_operand(image_operands);
        result.add_operands(image_operand_ids);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image(&mut self, result_type: &Inst, sampled_image: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE, result_id, Some(result_type));
        result.add_operand(sampled_image);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image_query_format(&mut self, result_type: &Inst, image: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE_QUERY_FORMAT, result_id, Some(result_type));
        result.add_operand(image);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image_query_order(&mut self, result_type: &Inst, image: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE_QUERY_ORDER, result_id, Some(result_type));
        result.add_operand(image);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image_query_size_lod(&mut self, result_type: &Inst, image: &Inst, levelof_detail: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE_QUERY_SIZE_LOD, result_id, Some(result_type));
        result.add_operand(image);
        result.add_operand(levelof_detail);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image_query_size(&mut self, result_type: &Inst, image: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE_QUERY_SIZE, result_id, Some(result_type));
        result.add_operand(image);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image_query_lod(&mut self, result_type: &Inst, sampled_image: &Inst, coordinate: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE_QUERY_LOD, result_id, Some(result_type));
        result.add_operand(sampled_image);
        result.add_operand(coordinate);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image_query_levels(&mut self, result_type: &Inst, image: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE_QUERY_LEVELS, result_id, Some(result_type));
        result.add_operand(image);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image_query_samples(&mut self, result_type: &Inst, image: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE_QUERY_SAMPLES, result_id, Some(result_type));
        result.add_operand(image);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image_sparse_sample_implicit_lod(&mut self, result_type: &Inst, sampled_image: &Inst, coordinate: &Inst, image_operands: ImageOperandsMask, image_operand_ids: &[Inst]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE_SPARSE_SAMPLE_IMPLICIT_LOD, result_id, Some(result_type));
        result.add_operand(sampled_image);
        result.add_operand(coordinate);
        result.add_operand(image_operands);
        result.add_operands(image_operand_ids);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image_sparse_sample_explicit_lod(&mut self, result_type: &Inst, sampled_image: &Inst, coordinate: &Inst, image_operands: ImageOperandsMask, image_operand_ids: &[Inst]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE_SPARSE_SAMPLE_EXPLICIT_LOD, result_id, Some(result_type));
        result.add_operand(sampled_image);
        result.add_operand(coordinate);
        result.add_operand(image_operands);
        result.add_operands(image_operand_ids);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image_sparse_sample_dref_implicit_lod(&mut self, result_type: &Inst, sampled_image: &Inst, coordinate: &Inst, d_ref: &Inst, image_operands: ImageOperandsMask, image_operand_ids: &[Inst]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE_SPARSE_SAMPLE_DREF_IMPLICIT_LOD, result_id, Some(result_type));
        result.add_operand(sampled_image);
        result.add_operand(coordinate);
        result.add_operand(d_ref);
        result.add_operand(image_operands);
        result.add_operands(image_operand_ids);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image_sparse_sample_dref_explicit_lod(&mut self, result_type: &Inst, sampled_image: &Inst, coordinate: &Inst, d_ref: &Inst, image_operands: ImageOperandsMask, image_operand_ids: &[Inst]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE_SPARSE_SAMPLE_DREF_EXPLICIT_LOD, result_id, Some(result_type));
        result.add_operand(sampled_image);
        result.add_operand(coordinate);
        result.add_operand(d_ref);
        result.add_operand(image_operands);
        result.add_operands(image_operand_ids);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image_sparse_sample_proj_implicit_lod(&mut self, result_type: &Inst, sampled_image: &Inst, coordinate: &Inst, image_operands: ImageOperandsMask, image_operand_ids: &[Inst]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE_SPARSE_SAMPLE_PROJ_IMPLICIT_LOD, result_id, Some(result_type));
        result.add_operand(sampled_image);
        result.add_operand(coordinate);
        result.add_operand(image_operands);
        result.add_operands(image_operand_ids);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image_sparse_sample_proj_explicit_lod(&mut self, result_type: &Inst, sampled_image: &Inst, coordinate: &Inst, image_operands: ImageOperandsMask, image_operand_ids: &[Inst]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE_SPARSE_SAMPLE_PROJ_EXPLICIT_LOD, result_id, Some(result_type));
        result.add_operand(sampled_image);
        result.add_operand(coordinate);
        result.add_operand(image_operands);
        result.add_operands(image_operand_ids);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image_sparse_sample_proj_dref_implicit_lod(&mut self, result_type: &Inst, sampled_image: &Inst, coordinate: &Inst, d_ref: &Inst, image_operands: ImageOperandsMask, image_operand_ids: &[Inst]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE_SPARSE_SAMPLE_PROJ_DREF_IMPLICIT_LOD, result_id, Some(result_type));
        result.add_operand(sampled_image);
        result.add_operand(coordinate);
        result.add_operand(d_ref);
        result.add_operand(image_operands);
        result.add_operands(image_operand_ids);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image_sparse_sample_proj_dref_explicit_lod(&mut self, result_type: &Inst, sampled_image: &Inst, coordinate: &Inst, d_ref: &Inst, image_operands: ImageOperandsMask, image_operand_ids: &[Inst]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE_SPARSE_SAMPLE_PROJ_DREF_EXPLICIT_LOD, result_id, Some(result_type));
        result.add_operand(sampled_image);
        result.add_operand(coordinate);
        result.add_operand(d_ref);
        result.add_operand(image_operands);
        result.add_operands(image_operand_ids);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image_sparse_fetch(&mut self, result_type: &Inst, image: &Inst, coordinate: &Inst, image_operands: ImageOperandsMask, image_operand_ids: &[Inst]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE_SPARSE_FETCH, result_id, Some(result_type));
        result.add_operand(image);
        result.add_operand(coordinate);
        result.add_operand(image_operands);
        result.add_operands(image_operand_ids);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image_sparse_gather(&mut self, result_type: &Inst, sampled_image: &Inst, coordinate: &Inst, component: &Inst, image_operands: ImageOperandsMask, image_operand_ids: &[Inst]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE_SPARSE_GATHER, result_id, Some(result_type));
        result.add_operand(sampled_image);
        result.add_operand(coordinate);
        result.add_operand(component);
        result.add_operand(image_operands);
        result.add_operands(image_operand_ids);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image_sparse_dref_gather(&mut self, result_type: &Inst, sampled_image: &Inst, coordinate: &Inst, d_ref: &Inst, image_operands: ImageOperandsMask, image_operand_ids: &[Inst]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE_SPARSE_DREF_GATHER, result_id, Some(result_type));
        result.add_operand(sampled_image);
        result.add_operand(coordinate);
        result.add_operand(d_ref);
        result.add_operand(image_operands);
        result.add_operands(image_operand_ids);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image_sparse_texels_resident(&mut self, result_type: &Inst, resident_code: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE_SPARSE_TEXELS_RESIDENT, result_id, Some(result_type));
        result.add_operand(resident_code);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image_sparse_read(&mut self, result_type: &Inst, image: &Inst, coordinate: &Inst, image_operands: ImageOperandsMask, image_operand_ids: &[Inst]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE_SPARSE_READ, result_id, Some(result_type));
        result.add_operand(image);
        result.add_operand(coordinate);
        result.add_operand(image_operands);
        result.add_operands(image_operand_ids);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn image_sample_footprint_nv(&mut self, result_type: &Inst, sampled_image: &Inst, coordinate: &Inst, granularity: &Inst, coarse: &Inst, image_operands: ImageOperandsMask, image_operand_ids: &[Inst]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IMAGE_SAMPLE_FOOTPRINT_NV, result_id, Some(result_type));
        result.add_operand(sampled_image);
        result.add_operand(coordinate);
        result.add_operand(granularity);
        result.add_operand(coarse);
        result.add_operand(image_operands);
        result.add_operands(image_operand_ids);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn convert_f_to_u(&mut self, result_type: &Inst, float_value: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::CONVERT_F_TO_U, result_id, Some(result_type));
        result.add_operand(float_value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn convert_f_to_s(&mut self, result_type: &Inst, float_value: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::CONVERT_F_TO_S, result_id, Some(result_type));
        result.add_operand(float_value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn convert_s_to_f(&mut self, result_type: &Inst, signed_value: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::CONVERT_S_TO_F, result_id, Some(result_type));
        result.add_operand(signed_value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn convert_u_to_f(&mut self, result_type: &Inst, unsigned_value: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::CONVERT_U_TO_F, result_id, Some(result_type));
        result.add_operand(unsigned_value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn u_convert(&mut self, result_type: &Inst, unsigned_value: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::U_CONVERT, result_id, Some(result_type));
        result.add_operand(unsigned_value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn s_convert(&mut self, result_type: &Inst, signed_value: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::S_CONVERT, result_id, Some(result_type));
        result.add_operand(signed_value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn f_convert(&mut self, result_type: &Inst, float_value: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::F_CONVERT, result_id, Some(result_type));
        result.add_operand(float_value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn quantize_to_f16(&mut self, result_type: &Inst, value: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::QUANTIZE_TO_F16, result_id, Some(result_type));
        result.add_operand(value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn convert_ptr_to_u(&mut self, result_type: &Inst, pointer: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::CONVERT_PTR_TO_U, result_id, Some(result_type));
        result.add_operand(pointer);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn sat_convert_s_to_u(&mut self, result_type: &Inst, signed_value: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::SAT_CONVERT_S_TO_U, result_id, Some(result_type));
        result.add_operand(signed_value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn sat_convert_u_to_s(&mut self, result_type: &Inst, unsigned_value: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::SAT_CONVERT_U_TO_S, result_id, Some(result_type));
        result.add_operand(unsigned_value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn convert_u_to_ptr(&mut self, result_type: &Inst, integer_value: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::CONVERT_U_TO_PTR, result_id, Some(result_type));
        result.add_operand(integer_value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn ptr_cast_to_generic(&mut self, result_type: &Inst, pointer: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::PTR_CAST_TO_GENERIC, result_id, Some(result_type));
        result.add_operand(pointer);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn generic_cast_to_ptr(&mut self, result_type: &Inst, pointer: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GENERIC_CAST_TO_PTR, result_id, Some(result_type));
        result.add_operand(pointer);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn generic_cast_to_ptr_explicit(&mut self, result_type: &Inst, pointer: &Inst, storage: StorageClass) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GENERIC_CAST_TO_PTR_EXPLICIT, result_id, Some(result_type));
        result.add_operand(pointer);
        result.add_operand(storage);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn bitcast(&mut self, result_type: &Inst, operand: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::BITCAST, result_id, Some(result_type));
        result.add_operand(operand);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn vector_extract_dynamic(&mut self, result_type: &Inst, vector: &Inst, index: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::VECTOR_EXTRACT_DYNAMIC, result_id, Some(result_type));
        result.add_operand(vector);
        result.add_operand(index);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn vector_insert_dynamic(&mut self, result_type: &Inst, vector: &Inst, component: &Inst, index: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::VECTOR_INSERT_DYNAMIC, result_id, Some(result_type));
        result.add_operand(vector);
        result.add_operand(component);
        result.add_operand(index);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn vector_shuffle(&mut self, result_type: &Inst, vector1: &Inst, vector2: &Inst, components: &[Operand]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::VECTOR_SHUFFLE, result_id, Some(result_type));
        result.add_operand(vector1);
        result.add_operand(vector2);
        result.add_operands(components);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn composite_construct(&mut self, result_type: &Inst, constituents: &[Inst]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::COMPOSITE_CONSTRUCT, result_id, Some(result_type));
        result.add_operands(constituents);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn composite_extract(&mut self, result_type: &Inst, composite: &Inst, indexes: &[Operand]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::COMPOSITE_EXTRACT, result_id, Some(result_type));
        result.add_operand(composite);
        result.add_operands(indexes);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn composite_insert(&mut self, result_type: &Inst, obj: &Inst, composite: &Inst, indexes: &[Operand]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::COMPOSITE_INSERT, result_id, Some(result_type));
        result.add_operand(obj);
        result.add_operand(composite);
        result.add_operands(indexes);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn copy_object(&mut self, result_type: &Inst, operand: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::COPY_OBJECT, result_id, Some(result_type));
        result.add_operand(operand);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn transpose(&mut self, result_type: &Inst, matrix: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::TRANSPOSE, result_id, Some(result_type));
        result.add_operand(matrix);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn copy_logical(&mut self, result_type: &Inst, operand: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::COPY_LOGICAL, result_id, Some(result_type));
        result.add_operand(operand);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn s_negate(&mut self, result_type: &Inst, operand: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::S_NEGATE, result_id, Some(result_type));
        result.add_operand(operand);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn f_negate(&mut self, result_type: &Inst, operand: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::F_NEGATE, result_id, Some(result_type));
        result.add_operand(operand);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn i_add(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::I_ADD, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn f_add(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::F_ADD, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn i_sub(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::I_SUB, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn f_sub(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::F_SUB, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn i_mul(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::I_MUL, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn f_mul(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::F_MUL, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn u_div(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::U_DIV, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn s_div(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::S_DIV, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn f_div(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::F_DIV, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn u_mod(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::U_MOD, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn s_rem(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::S_REM, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn s_mod(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::S_MOD, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn f_rem(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::F_REM, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn f_mod(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::F_MOD, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn vector_times_scalar(&mut self, result_type: &Inst, vector: &Inst, scalar: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::VECTOR_TIMES_SCALAR, result_id, Some(result_type));
        result.add_operand(vector);
        result.add_operand(scalar);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn matrix_times_scalar(&mut self, result_type: &Inst, matrix: &Inst, scalar: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::MATRIX_TIMES_SCALAR, result_id, Some(result_type));
        result.add_operand(matrix);
        result.add_operand(scalar);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn vector_times_matrix(&mut self, result_type: &Inst, vector: &Inst, matrix: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::VECTOR_TIMES_MATRIX, result_id, Some(result_type));
        result.add_operand(vector);
        result.add_operand(matrix);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn matrix_times_vector(&mut self, result_type: &Inst, matrix: &Inst, vector: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::MATRIX_TIMES_VECTOR, result_id, Some(result_type));
        result.add_operand(matrix);
        result.add_operand(vector);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn matrix_times_matrix(&mut self, result_type: &Inst, left_matrix: &Inst, right_matrix: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::MATRIX_TIMES_MATRIX, result_id, Some(result_type));
        result.add_operand(left_matrix);
        result.add_operand(right_matrix);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn outer_product(&mut self, result_type: &Inst, vector1: &Inst, vector2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::OUTER_PRODUCT, result_id, Some(result_type));
        result.add_operand(vector1);
        result.add_operand(vector2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn dot(&mut self, result_type: &Inst, vector1: &Inst, vector2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::DOT, result_id, Some(result_type));
        result.add_operand(vector1);
        result.add_operand(vector2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn i_add_carry(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::I_ADD_CARRY, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn i_sub_borrow(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::I_SUB_BORROW, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn u_mul_extended(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::U_MUL_EXTENDED, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn s_mul_extended(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::S_MUL_EXTENDED, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn shift_right_logical(&mut self, result_type: &Inst, base_obj: &Inst, shift: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::SHIFT_RIGHT_LOGICAL, result_id, Some(result_type));
        result.add_operand(base_obj);
        result.add_operand(shift);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn shift_right_arithmetic(&mut self, result_type: &Inst, base_obj: &Inst, shift: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::SHIFT_RIGHT_ARITHMETIC, result_id, Some(result_type));
        result.add_operand(base_obj);
        result.add_operand(shift);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn shift_left_logical(&mut self, result_type: &Inst, base_obj: &Inst, shift: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::SHIFT_LEFT_LOGICAL, result_id, Some(result_type));
        result.add_operand(base_obj);
        result.add_operand(shift);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn bitwise_or(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::BITWISE_OR, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn bitwise_xor(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::BITWISE_XOR, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn bitwise_and(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::BITWISE_AND, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn not(&mut self, result_type: &Inst, operand: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::NOT, result_id, Some(result_type));
        result.add_operand(operand);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn bit_field_insert(&mut self, result_type: &Inst, base_obj: &Inst, insert: &Inst, offset: &Inst, count: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::BIT_FIELD_INSERT, result_id, Some(result_type));
        result.add_operand(base_obj);
        result.add_operand(insert);
        result.add_operand(offset);
        result.add_operand(count);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn bit_field_s_extract(&mut self, result_type: &Inst, base_obj: &Inst, offset: &Inst, count: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::BIT_FIELD_S_EXTRACT, result_id, Some(result_type));
        result.add_operand(base_obj);
        result.add_operand(offset);
        result.add_operand(count);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn bit_field_u_extract(&mut self, result_type: &Inst, base_obj: &Inst, offset: &Inst, count: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::BIT_FIELD_U_EXTRACT, result_id, Some(result_type));
        result.add_operand(base_obj);
        result.add_operand(offset);
        result.add_operand(count);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn bit_reverse(&mut self, result_type: &Inst, base_obj: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::BIT_REVERSE, result_id, Some(result_type));
        result.add_operand(base_obj);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn bit_count(&mut self, result_type: &Inst, base_obj: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::BIT_COUNT, result_id, Some(result_type));
        result.add_operand(base_obj);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn any(&mut self, result_type: &Inst, vector: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::ANY, result_id, Some(result_type));
        result.add_operand(vector);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn all(&mut self, result_type: &Inst, vector: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::ALL, result_id, Some(result_type));
        result.add_operand(vector);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn is_nan(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IS_NAN, result_id, Some(result_type));
        result.add_operand(x);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn is_inf(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IS_INF, result_id, Some(result_type));
        result.add_operand(x);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn is_finite(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IS_FINITE, result_id, Some(result_type));
        result.add_operand(x);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn is_normal(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IS_NORMAL, result_id, Some(result_type));
        result.add_operand(x);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn sign_bit_set(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::SIGN_BIT_SET, result_id, Some(result_type));
        result.add_operand(x);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn less_or_greater(&mut self, result_type: &Inst, x: &Inst, y: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::LESS_OR_GREATER, result_id, Some(result_type));
        result.add_operand(x);
        result.add_operand(y);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn ordered(&mut self, result_type: &Inst, x: &Inst, y: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::ORDERED, result_id, Some(result_type));
        result.add_operand(x);
        result.add_operand(y);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn unordered(&mut self, result_type: &Inst, x: &Inst, y: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::UNORDERED, result_id, Some(result_type));
        result.add_operand(x);
        result.add_operand(y);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn logical_equal(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::LOGICAL_EQUAL, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn logical_not_equal(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::LOGICAL_NOT_EQUAL, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn logical_or(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::LOGICAL_OR, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn logical_and(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::LOGICAL_AND, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn logical_not(&mut self, result_type: &Inst, operand: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::LOGICAL_NOT, result_id, Some(result_type));
        result.add_operand(operand);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn select(&mut self, result_type: &Inst, condition: &Inst, object1: &Inst, object2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::SELECT, result_id, Some(result_type));
        result.add_operand(condition);
        result.add_operand(object1);
        result.add_operand(object2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn i_equal(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::I_EQUAL, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn i_not_equal(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::I_NOT_EQUAL, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn u_greater_than(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::U_GREATER_THAN, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn s_greater_than(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::S_GREATER_THAN, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn u_greater_than_equal(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::U_GREATER_THAN_EQUAL, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn s_greater_than_equal(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::S_GREATER_THAN_EQUAL, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn u_less_than(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::U_LESS_THAN, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn s_less_than(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::S_LESS_THAN, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn u_less_than_equal(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::U_LESS_THAN_EQUAL, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn s_less_than_equal(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::S_LESS_THAN_EQUAL, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn f_ord_equal(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::F_ORD_EQUAL, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn f_unord_equal(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::F_UNORD_EQUAL, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn f_ord_not_equal(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::F_ORD_NOT_EQUAL, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn f_unord_not_equal(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::F_UNORD_NOT_EQUAL, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn f_ord_less_than(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::F_ORD_LESS_THAN, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn f_unord_less_than(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::F_UNORD_LESS_THAN, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn f_ord_greater_than(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::F_ORD_GREATER_THAN, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn f_unord_greater_than(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::F_UNORD_GREATER_THAN, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn f_ord_less_than_equal(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::F_ORD_LESS_THAN_EQUAL, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn f_unord_less_than_equal(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::F_UNORD_LESS_THAN_EQUAL, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn f_ord_greater_than_equal(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::F_ORD_GREATER_THAN_EQUAL, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn f_unord_greater_than_equal(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::F_UNORD_GREATER_THAN_EQUAL, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn d_pdx(&mut self, result_type: &Inst, p: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::D_PDX, result_id, Some(result_type));
        result.add_operand(p);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn d_pdy(&mut self, result_type: &Inst, p: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::D_PDY, result_id, Some(result_type));
        result.add_operand(p);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn fwidth(&mut self, result_type: &Inst, p: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::FWIDTH, result_id, Some(result_type));
        result.add_operand(p);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn d_pdx_fine(&mut self, result_type: &Inst, p: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::D_PDX_FINE, result_id, Some(result_type));
        result.add_operand(p);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn d_pdy_fine(&mut self, result_type: &Inst, p: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::D_PDY_FINE, result_id, Some(result_type));
        result.add_operand(p);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn fwidth_fine(&mut self, result_type: &Inst, p: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::FWIDTH_FINE, result_id, Some(result_type));
        result.add_operand(p);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn d_pdx_coarse(&mut self, result_type: &Inst, p: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::D_PDX_COARSE, result_id, Some(result_type));
        result.add_operand(p);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn d_pdy_coarse(&mut self, result_type: &Inst, p: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::D_PDY_COARSE, result_id, Some(result_type));
        result.add_operand(p);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn fwidth_coarse(&mut self, result_type: &Inst, p: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::FWIDTH_COARSE, result_id, Some(result_type));
        result.add_operand(p);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn phi(&mut self, result_type: &Inst, parameters: &[Inst]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::PHI, result_id, Some(result_type));
        result.add_operands(parameters);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn loop_merge(&mut self, merge_block: &Inst, continue_target: &Inst, loop_control: LoopControlMask) -> Inst {
        let result = self.new_instruction(Op::LOOP_MERGE, INVALID_ID, None);
        result.add_operand(merge_block);
        result.add_operand(continue_target);
        result.add_operand(loop_control);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn selection_merge(&mut self, merge_block: &Inst, selection_control: SelectionControlMask) -> Inst {
        let result = self.new_instruction(Op::SELECTION_MERGE, INVALID_ID, None);
        result.add_operand(merge_block);
        result.add_operand(selection_control);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn label(&mut self, ) -> Inst {
        let result = self.new_instruction(Op::LABEL, INVALID_ID, None);
        result
    }

    pub fn branch(&mut self, target_label: &Inst) -> Inst {
        let result = self.new_instruction(Op::BRANCH, INVALID_ID, None);
        result.add_operand(target_label);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn branch_conditional(&mut self, condition: &Inst, true_label: &Inst, false_label: &Inst, branchweights: &[Operand]) -> Inst {
        let result = self.new_instruction(Op::BRANCH_CONDITIONAL, INVALID_ID, None);
        result.add_operand(condition);
        result.add_operand(true_label);
        result.add_operand(false_label);
        result.add_operands(branchweights);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn switch(&mut self, selector: &Inst, default_obj: &Inst, target: &[Operand]) -> Inst {
        let result = self.new_instruction(Op::SWITCH, INVALID_ID, None);
        result.add_operand(selector);
        result.add_operand(default_obj);
        result.add_operands(target);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn kill(&mut self, ) -> Inst {
        let result = self.new_instruction(Op::KILL, INVALID_ID, None);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn return_(&mut self, ) -> Inst {
        let result = self.new_instruction(Op::RETURN, INVALID_ID, None);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn return_value(&mut self, value: &Inst) -> Inst {
        let result = self.new_instruction(Op::RETURN_VALUE, INVALID_ID, None);
        result.add_operand(value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn unreachable(&mut self, ) -> Inst {
        let result = self.new_instruction(Op::UNREACHABLE, INVALID_ID, None);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn lifetime_start(&mut self, pointer: &Inst, size: Lit) -> Inst {
        let result = self.new_instruction(Op::LIFETIME_START, INVALID_ID, None);
        result.add_operand(pointer);
        result.add_operand(size);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn lifetime_stop(&mut self, pointer: &Inst, size: Lit) -> Inst {
        let result = self.new_instruction(Op::LIFETIME_STOP, INVALID_ID, None);
        result.add_operand(pointer);
        result.add_operand(size);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn terminate_invocation(&mut self, ) -> Inst {
        let result = self.new_instruction(Op::TERMINATE_INVOCATION, INVALID_ID, None);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn atomic_load(&mut self, result_type: &Inst, pointer: &Inst, memory: &Inst, semantics: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::ATOMIC_LOAD, result_id, Some(result_type));
        result.add_operand(pointer);
        result.add_operand(memory);
        result.add_operand(semantics);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn atomic_store(&mut self, pointer: &Inst, memory: &Inst, semantics: &Inst, value: &Inst) -> Inst {
        let result = self.new_instruction(Op::ATOMIC_STORE, INVALID_ID, None);
        result.add_operand(pointer);
        result.add_operand(memory);
        result.add_operand(semantics);
        result.add_operand(value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn atomic_exchange(&mut self, result_type: &Inst, pointer: &Inst, memory: &Inst, semantics: &Inst, value: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::ATOMIC_EXCHANGE, result_id, Some(result_type));
        result.add_operand(pointer);
        result.add_operand(memory);
        result.add_operand(semantics);
        result.add_operand(value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn atomic_compare_exchange(&mut self, result_type: &Inst, pointer: &Inst, memory: &Inst, equal: &Inst, unequal: &Inst, value: &Inst, comparator: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::ATOMIC_COMPARE_EXCHANGE, result_id, Some(result_type));
        result.add_operand(pointer);
        result.add_operand(memory);
        result.add_operand(equal);
        result.add_operand(unequal);
        result.add_operand(value);
        result.add_operand(comparator);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn atomic_compare_exchange_weak(&mut self, result_type: &Inst, pointer: &Inst, memory: &Inst, equal: &Inst, unequal: &Inst, value: &Inst, comparator: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::ATOMIC_COMPARE_EXCHANGE_WEAK, result_id, Some(result_type));
        result.add_operand(pointer);
        result.add_operand(memory);
        result.add_operand(equal);
        result.add_operand(unequal);
        result.add_operand(value);
        result.add_operand(comparator);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn atomic_i_increment(&mut self, result_type: &Inst, pointer: &Inst, memory: &Inst, semantics: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::ATOMIC_I_INCREMENT, result_id, Some(result_type));
        result.add_operand(pointer);
        result.add_operand(memory);
        result.add_operand(semantics);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn atomic_i_decrement(&mut self, result_type: &Inst, pointer: &Inst, memory: &Inst, semantics: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::ATOMIC_I_DECREMENT, result_id, Some(result_type));
        result.add_operand(pointer);
        result.add_operand(memory);
        result.add_operand(semantics);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn atomic_i_add(&mut self, result_type: &Inst, pointer: &Inst, memory: &Inst, semantics: &Inst, value: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::ATOMIC_I_ADD, result_id, Some(result_type));
        result.add_operand(pointer);
        result.add_operand(memory);
        result.add_operand(semantics);
        result.add_operand(value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn atomic_i_sub(&mut self, result_type: &Inst, pointer: &Inst, memory: &Inst, semantics: &Inst, value: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::ATOMIC_I_SUB, result_id, Some(result_type));
        result.add_operand(pointer);
        result.add_operand(memory);
        result.add_operand(semantics);
        result.add_operand(value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn atomic_s_min(&mut self, result_type: &Inst, pointer: &Inst, memory: &Inst, semantics: &Inst, value: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::ATOMIC_S_MIN, result_id, Some(result_type));
        result.add_operand(pointer);
        result.add_operand(memory);
        result.add_operand(semantics);
        result.add_operand(value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn atomic_u_min(&mut self, result_type: &Inst, pointer: &Inst, memory: &Inst, semantics: &Inst, value: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::ATOMIC_U_MIN, result_id, Some(result_type));
        result.add_operand(pointer);
        result.add_operand(memory);
        result.add_operand(semantics);
        result.add_operand(value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn atomic_s_max(&mut self, result_type: &Inst, pointer: &Inst, memory: &Inst, semantics: &Inst, value: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::ATOMIC_S_MAX, result_id, Some(result_type));
        result.add_operand(pointer);
        result.add_operand(memory);
        result.add_operand(semantics);
        result.add_operand(value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn atomic_u_max(&mut self, result_type: &Inst, pointer: &Inst, memory: &Inst, semantics: &Inst, value: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::ATOMIC_U_MAX, result_id, Some(result_type));
        result.add_operand(pointer);
        result.add_operand(memory);
        result.add_operand(semantics);
        result.add_operand(value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn atomic_and(&mut self, result_type: &Inst, pointer: &Inst, memory: &Inst, semantics: &Inst, value: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::ATOMIC_AND, result_id, Some(result_type));
        result.add_operand(pointer);
        result.add_operand(memory);
        result.add_operand(semantics);
        result.add_operand(value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn atomic_or(&mut self, result_type: &Inst, pointer: &Inst, memory: &Inst, semantics: &Inst, value: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::ATOMIC_OR, result_id, Some(result_type));
        result.add_operand(pointer);
        result.add_operand(memory);
        result.add_operand(semantics);
        result.add_operand(value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn atomic_xor(&mut self, result_type: &Inst, pointer: &Inst, memory: &Inst, semantics: &Inst, value: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::ATOMIC_XOR, result_id, Some(result_type));
        result.add_operand(pointer);
        result.add_operand(memory);
        result.add_operand(semantics);
        result.add_operand(value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn atomic_flag_test_and_set(&mut self, result_type: &Inst, pointer: &Inst, memory: &Inst, semantics: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::ATOMIC_FLAG_TEST_AND_SET, result_id, Some(result_type));
        result.add_operand(pointer);
        result.add_operand(memory);
        result.add_operand(semantics);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn atomic_flag_clear(&mut self, pointer: &Inst, memory: &Inst, semantics: &Inst) -> Inst {
        let result = self.new_instruction(Op::ATOMIC_FLAG_CLEAR, INVALID_ID, None);
        result.add_operand(pointer);
        result.add_operand(memory);
        result.add_operand(semantics);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn atomic_f_add_ext(&mut self, result_type: &Inst, pointer: &Inst, memory: &Inst, semantics: &Inst, value: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::ATOMIC_F_ADD_EXT, result_id, Some(result_type));
        result.add_operand(pointer);
        result.add_operand(memory);
        result.add_operand(semantics);
        result.add_operand(value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn emit_vertex(&mut self, ) -> Inst {
        let result = self.new_instruction(Op::EMIT_VERTEX, INVALID_ID, None);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn end_primitive(&mut self, ) -> Inst {
        let result = self.new_instruction(Op::END_PRIMITIVE, INVALID_ID, None);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn emit_stream_vertex(&mut self, stream: &Inst) -> Inst {
        let result = self.new_instruction(Op::EMIT_STREAM_VERTEX, INVALID_ID, None);
        result.add_operand(stream);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn end_stream_primitive(&mut self, stream: &Inst) -> Inst {
        let result = self.new_instruction(Op::END_STREAM_PRIMITIVE, INVALID_ID, None);
        result.add_operand(stream);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn control_barrier(&mut self, execution: &Inst, memory: &Inst, semantics: &Inst) -> Inst {
        let result = self.new_instruction(Op::CONTROL_BARRIER, INVALID_ID, None);
        result.add_operand(execution);
        result.add_operand(memory);
        result.add_operand(semantics);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn memory_barrier(&mut self, memory: &Inst, semantics: &Inst) -> Inst {
        let result = self.new_instruction(Op::MEMORY_BARRIER, INVALID_ID, None);
        result.add_operand(memory);
        result.add_operand(semantics);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn named_barrier_initialize(&mut self, result_type: &Inst, subgroup_count: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::NAMED_BARRIER_INITIALIZE, result_id, Some(result_type));
        result.add_operand(subgroup_count);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn memory_named_barrier(&mut self, named_barrier: &Inst, memory: &Inst, semantics: &Inst) -> Inst {
        let result = self.new_instruction(Op::MEMORY_NAMED_BARRIER, INVALID_ID, None);
        result.add_operand(named_barrier);
        result.add_operand(memory);
        result.add_operand(semantics);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_async_copy(&mut self, result_type: &Inst, execution: &Inst, destination: &Inst, source: &Inst, num_elements: &Inst, stride: &Inst, event_obj: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_ASYNC_COPY, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(destination);
        result.add_operand(source);
        result.add_operand(num_elements);
        result.add_operand(stride);
        result.add_operand(event_obj);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_wait_events(&mut self, execution: &Inst, num_events: &Inst, events_list: &Inst) -> Inst {
        let result = self.new_instruction(Op::GROUP_WAIT_EVENTS, INVALID_ID, None);
        result.add_operand(execution);
        result.add_operand(num_events);
        result.add_operand(events_list);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_all(&mut self, result_type: &Inst, execution: &Inst, predicate: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_ALL, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(predicate);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_any(&mut self, result_type: &Inst, execution: &Inst, predicate: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_ANY, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(predicate);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_broadcast(&mut self, result_type: &Inst, execution: &Inst, value: &Inst, local_id: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_BROADCAST, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(value);
        result.add_operand(local_id);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_i_add(&mut self, result_type: &Inst, execution: &Inst, operation: GroupOperation, x: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_I_ADD, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(operation);
        result.add_operand(x);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_f_add(&mut self, result_type: &Inst, execution: &Inst, operation: GroupOperation, x: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_F_ADD, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(operation);
        result.add_operand(x);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_f_min(&mut self, result_type: &Inst, execution: &Inst, operation: GroupOperation, x: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_F_MIN, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(operation);
        result.add_operand(x);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_u_min(&mut self, result_type: &Inst, execution: &Inst, operation: GroupOperation, x: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_U_MIN, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(operation);
        result.add_operand(x);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_s_min(&mut self, result_type: &Inst, execution: &Inst, operation: GroupOperation, x: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_S_MIN, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(operation);
        result.add_operand(x);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_f_max(&mut self, result_type: &Inst, execution: &Inst, operation: GroupOperation, x: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_F_MAX, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(operation);
        result.add_operand(x);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_u_max(&mut self, result_type: &Inst, execution: &Inst, operation: GroupOperation, x: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_U_MAX, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(operation);
        result.add_operand(x);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_s_max(&mut self, result_type: &Inst, execution: &Inst, operation: GroupOperation, x: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_S_MAX, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(operation);
        result.add_operand(x);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn subgroup_ballot_khr(&mut self, result_type: &Inst, predicate: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::SUBGROUP_BALLOT_KHR, result_id, Some(result_type));
        result.add_operand(predicate);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn subgroup_first_invocation_khr(&mut self, result_type: &Inst, value: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::SUBGROUP_FIRST_INVOCATION_KHR, result_id, Some(result_type));
        result.add_operand(value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn subgroup_all_khr(&mut self, result_type: &Inst, predicate: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::SUBGROUP_ALL_KHR, result_id, Some(result_type));
        result.add_operand(predicate);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn subgroup_any_khr(&mut self, result_type: &Inst, predicate: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::SUBGROUP_ANY_KHR, result_id, Some(result_type));
        result.add_operand(predicate);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn subgroup_all_equal_khr(&mut self, result_type: &Inst, predicate: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::SUBGROUP_ALL_EQUAL_KHR, result_id, Some(result_type));
        result.add_operand(predicate);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn subgroup_read_invocation_khr(&mut self, result_type: &Inst, value: &Inst, index: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::SUBGROUP_READ_INVOCATION_KHR, result_id, Some(result_type));
        result.add_operand(value);
        result.add_operand(index);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_i_add_non_uniform_amd(&mut self, result_type: &Inst, execution: &Inst, operation: GroupOperation, x: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_I_ADD_NON_UNIFORM_AMD, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(operation);
        result.add_operand(x);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_f_add_non_uniform_amd(&mut self, result_type: &Inst, execution: &Inst, operation: GroupOperation, x: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_F_ADD_NON_UNIFORM_AMD, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(operation);
        result.add_operand(x);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_f_min_non_uniform_amd(&mut self, result_type: &Inst, execution: &Inst, operation: GroupOperation, x: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_F_MIN_NON_UNIFORM_AMD, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(operation);
        result.add_operand(x);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_u_min_non_uniform_amd(&mut self, result_type: &Inst, execution: &Inst, operation: GroupOperation, x: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_U_MIN_NON_UNIFORM_AMD, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(operation);
        result.add_operand(x);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_s_min_non_uniform_amd(&mut self, result_type: &Inst, execution: &Inst, operation: GroupOperation, x: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_S_MIN_NON_UNIFORM_AMD, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(operation);
        result.add_operand(x);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_f_max_non_uniform_amd(&mut self, result_type: &Inst, execution: &Inst, operation: GroupOperation, x: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_F_MAX_NON_UNIFORM_AMD, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(operation);
        result.add_operand(x);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_u_max_non_uniform_amd(&mut self, result_type: &Inst, execution: &Inst, operation: GroupOperation, x: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_U_MAX_NON_UNIFORM_AMD, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(operation);
        result.add_operand(x);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_s_max_non_uniform_amd(&mut self, result_type: &Inst, execution: &Inst, operation: GroupOperation, x: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_S_MAX_NON_UNIFORM_AMD, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(operation);
        result.add_operand(x);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn subgroup_shuffle_intel(&mut self, result_type: &Inst, data: &Inst, invocation_id: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::SUBGROUP_SHUFFLE_INTEL, result_id, Some(result_type));
        result.add_operand(data);
        result.add_operand(invocation_id);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn subgroup_shuffle_down_intel(&mut self, result_type: &Inst, current: &Inst, next: &Inst, delta: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::SUBGROUP_SHUFFLE_DOWN_INTEL, result_id, Some(result_type));
        result.add_operand(current);
        result.add_operand(next);
        result.add_operand(delta);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn subgroup_shuffle_up_intel(&mut self, result_type: &Inst, previous: &Inst, current: &Inst, delta: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::SUBGROUP_SHUFFLE_UP_INTEL, result_id, Some(result_type));
        result.add_operand(previous);
        result.add_operand(current);
        result.add_operand(delta);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn subgroup_shuffle_xor_intel(&mut self, result_type: &Inst, data: &Inst, value: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::SUBGROUP_SHUFFLE_XOR_INTEL, result_id, Some(result_type));
        result.add_operand(data);
        result.add_operand(value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn subgroup_block_read_intel(&mut self, result_type: &Inst, ptr: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::SUBGROUP_BLOCK_READ_INTEL, result_id, Some(result_type));
        result.add_operand(ptr);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn subgroup_block_write_intel(&mut self, ptr: &Inst, data: &Inst) -> Inst {
        let result = self.new_instruction(Op::SUBGROUP_BLOCK_WRITE_INTEL, INVALID_ID, None);
        result.add_operand(ptr);
        result.add_operand(data);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn subgroup_image_block_read_intel(&mut self, result_type: &Inst, image: &Inst, coordinate: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::SUBGROUP_IMAGE_BLOCK_READ_INTEL, result_id, Some(result_type));
        result.add_operand(image);
        result.add_operand(coordinate);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn subgroup_image_block_write_intel(&mut self, image: &Inst, coordinate: &Inst, data: &Inst) -> Inst {
        let result = self.new_instruction(Op::SUBGROUP_IMAGE_BLOCK_WRITE_INTEL, INVALID_ID, None);
        result.add_operand(image);
        result.add_operand(coordinate);
        result.add_operand(data);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn subgroup_image_media_block_read_intel(&mut self, result_type: &Inst, image: &Inst, coordinate: &Inst, width: &Inst, height: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::SUBGROUP_IMAGE_MEDIA_BLOCK_READ_INTEL, result_id, Some(result_type));
        result.add_operand(image);
        result.add_operand(coordinate);
        result.add_operand(width);
        result.add_operand(height);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn subgroup_image_media_block_write_intel(&mut self, image: &Inst, coordinate: &Inst, width: &Inst, height: &Inst, data: &Inst) -> Inst {
        let result = self.new_instruction(Op::SUBGROUP_IMAGE_MEDIA_BLOCK_WRITE_INTEL, INVALID_ID, None);
        result.add_operand(image);
        result.add_operand(coordinate);
        result.add_operand(width);
        result.add_operand(height);
        result.add_operand(data);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn enqueue_marker(&mut self, result_type: &Inst, queue: &Inst, num_events: &Inst, wait_events: &Inst, ret_event: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::ENQUEUE_MARKER, result_id, Some(result_type));
        result.add_operand(queue);
        result.add_operand(num_events);
        result.add_operand(wait_events);
        result.add_operand(ret_event);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn enqueue_kernel(&mut self, result_type: &Inst, queue: &Inst, flags: &Inst, n_d_range: &Inst, num_events: &Inst, wait_events: &Inst, ret_event: &Inst, invoke: &Inst, param: &Inst, param_size: &Inst, param_align: &Inst, local_size: &[Inst]) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::ENQUEUE_KERNEL, result_id, Some(result_type));
        result.add_operand(queue);
        result.add_operand(flags);
        result.add_operand(n_d_range);
        result.add_operand(num_events);
        result.add_operand(wait_events);
        result.add_operand(ret_event);
        result.add_operand(invoke);
        result.add_operand(param);
        result.add_operand(param_size);
        result.add_operand(param_align);
        result.add_operands(local_size);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn get_kernel_n_drange_sub_group_count(&mut self, result_type: &Inst, n_d_range: &Inst, invoke: &Inst, param: &Inst, param_size: &Inst, param_align: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GET_KERNEL_N_DRANGE_SUB_GROUP_COUNT, result_id, Some(result_type));
        result.add_operand(n_d_range);
        result.add_operand(invoke);
        result.add_operand(param);
        result.add_operand(param_size);
        result.add_operand(param_align);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn get_kernel_n_drange_max_sub_group_size(&mut self, result_type: &Inst, n_d_range: &Inst, invoke: &Inst, param: &Inst, param_size: &Inst, param_align: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GET_KERNEL_N_DRANGE_MAX_SUB_GROUP_SIZE, result_id, Some(result_type));
        result.add_operand(n_d_range);
        result.add_operand(invoke);
        result.add_operand(param);
        result.add_operand(param_size);
        result.add_operand(param_align);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn get_kernel_work_group_size(&mut self, result_type: &Inst, invoke: &Inst, param: &Inst, param_size: &Inst, param_align: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GET_KERNEL_WORK_GROUP_SIZE, result_id, Some(result_type));
        result.add_operand(invoke);
        result.add_operand(param);
        result.add_operand(param_size);
        result.add_operand(param_align);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn get_kernel_preferred_work_group_size_multiple(&mut self, result_type: &Inst, invoke: &Inst, param: &Inst, param_size: &Inst, param_align: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GET_KERNEL_PREFERRED_WORK_GROUP_SIZE_MULTIPLE, result_id, Some(result_type));
        result.add_operand(invoke);
        result.add_operand(param);
        result.add_operand(param_size);
        result.add_operand(param_align);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn retain_event(&mut self, event_obj: &Inst) -> Inst {
        let result = self.new_instruction(Op::RETAIN_EVENT, INVALID_ID, None);
        result.add_operand(event_obj);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn release_event(&mut self, event_obj: &Inst) -> Inst {
        let result = self.new_instruction(Op::RELEASE_EVENT, INVALID_ID, None);
        result.add_operand(event_obj);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn create_user_event(&mut self, result_type: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::CREATE_USER_EVENT, result_id, Some(result_type));
        self.add_to_function_definitions(&result);
        result
    }

    pub fn is_valid_event(&mut self, result_type: &Inst, event_obj: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IS_VALID_EVENT, result_id, Some(result_type));
        result.add_operand(event_obj);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn set_user_event_status(&mut self, event_obj: &Inst, status: &Inst) -> Inst {
        let result = self.new_instruction(Op::SET_USER_EVENT_STATUS, INVALID_ID, None);
        result.add_operand(event_obj);
        result.add_operand(status);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn capture_event_profiling_info(&mut self, event_obj: &Inst, profiling_info: &Inst, value: &Inst) -> Inst {
        let result = self.new_instruction(Op::CAPTURE_EVENT_PROFILING_INFO, INVALID_ID, None);
        result.add_operand(event_obj);
        result.add_operand(profiling_info);
        result.add_operand(value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn get_default_queue(&mut self, result_type: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GET_DEFAULT_QUEUE, result_id, Some(result_type));
        self.add_to_function_definitions(&result);
        result
    }

    pub fn build_nd_range(&mut self, result_type: &Inst, global_work_size: &Inst, local_work_size: &Inst, global_work_offset: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::BUILD_ND_RANGE, result_id, Some(result_type));
        result.add_operand(global_work_size);
        result.add_operand(local_work_size);
        result.add_operand(global_work_offset);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn get_kernel_local_size_for_subgroup_count(&mut self, result_type: &Inst, subgroup_count: &Inst, invoke: &Inst, param: &Inst, param_size: &Inst, param_align: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GET_KERNEL_LOCAL_SIZE_FOR_SUBGROUP_COUNT, result_id, Some(result_type));
        result.add_operand(subgroup_count);
        result.add_operand(invoke);
        result.add_operand(param);
        result.add_operand(param_size);
        result.add_operand(param_align);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn get_kernel_max_num_subgroups(&mut self, result_type: &Inst, invoke: &Inst, param: &Inst, param_size: &Inst, param_align: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GET_KERNEL_MAX_NUM_SUBGROUPS, result_id, Some(result_type));
        result.add_operand(invoke);
        result.add_operand(param);
        result.add_operand(param_size);
        result.add_operand(param_align);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn read_pipe(&mut self, result_type: &Inst, pipe: &Inst, pointer: &Inst, packet_size: &Inst, packet_alignment: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::READ_PIPE, result_id, Some(result_type));
        result.add_operand(pipe);
        result.add_operand(pointer);
        result.add_operand(packet_size);
        result.add_operand(packet_alignment);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn write_pipe(&mut self, result_type: &Inst, pipe: &Inst, pointer: &Inst, packet_size: &Inst, packet_alignment: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::WRITE_PIPE, result_id, Some(result_type));
        result.add_operand(pipe);
        result.add_operand(pointer);
        result.add_operand(packet_size);
        result.add_operand(packet_alignment);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn reserved_read_pipe(&mut self, result_type: &Inst, pipe: &Inst, reserve_id: &Inst, index: &Inst, pointer: &Inst, packet_size: &Inst, packet_alignment: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::RESERVED_READ_PIPE, result_id, Some(result_type));
        result.add_operand(pipe);
        result.add_operand(reserve_id);
        result.add_operand(index);
        result.add_operand(pointer);
        result.add_operand(packet_size);
        result.add_operand(packet_alignment);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn reserved_write_pipe(&mut self, result_type: &Inst, pipe: &Inst, reserve_id: &Inst, index: &Inst, pointer: &Inst, packet_size: &Inst, packet_alignment: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::RESERVED_WRITE_PIPE, result_id, Some(result_type));
        result.add_operand(pipe);
        result.add_operand(reserve_id);
        result.add_operand(index);
        result.add_operand(pointer);
        result.add_operand(packet_size);
        result.add_operand(packet_alignment);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn reserve_read_pipe_packets(&mut self, result_type: &Inst, pipe: &Inst, num_packets: &Inst, packet_size: &Inst, packet_alignment: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::RESERVE_READ_PIPE_PACKETS, result_id, Some(result_type));
        result.add_operand(pipe);
        result.add_operand(num_packets);
        result.add_operand(packet_size);
        result.add_operand(packet_alignment);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn reserve_write_pipe_packets(&mut self, result_type: &Inst, pipe: &Inst, num_packets: &Inst, packet_size: &Inst, packet_alignment: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::RESERVE_WRITE_PIPE_PACKETS, result_id, Some(result_type));
        result.add_operand(pipe);
        result.add_operand(num_packets);
        result.add_operand(packet_size);
        result.add_operand(packet_alignment);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn commit_read_pipe(&mut self, pipe: &Inst, reserve_id: &Inst, packet_size: &Inst, packet_alignment: &Inst) -> Inst {
        let result = self.new_instruction(Op::COMMIT_READ_PIPE, INVALID_ID, None);
        result.add_operand(pipe);
        result.add_operand(reserve_id);
        result.add_operand(packet_size);
        result.add_operand(packet_alignment);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn commit_write_pipe(&mut self, pipe: &Inst, reserve_id: &Inst, packet_size: &Inst, packet_alignment: &Inst) -> Inst {
        let result = self.new_instruction(Op::COMMIT_WRITE_PIPE, INVALID_ID, None);
        result.add_operand(pipe);
        result.add_operand(reserve_id);
        result.add_operand(packet_size);
        result.add_operand(packet_alignment);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn is_valid_reserve_id(&mut self, result_type: &Inst, reserve_id: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IS_VALID_RESERVE_ID, result_id, Some(result_type));
        result.add_operand(reserve_id);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn get_num_pipe_packets(&mut self, result_type: &Inst, pipe: &Inst, packet_size: &Inst, packet_alignment: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GET_NUM_PIPE_PACKETS, result_id, Some(result_type));
        result.add_operand(pipe);
        result.add_operand(packet_size);
        result.add_operand(packet_alignment);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn get_max_pipe_packets(&mut self, result_type: &Inst, pipe: &Inst, packet_size: &Inst, packet_alignment: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GET_MAX_PIPE_PACKETS, result_id, Some(result_type));
        result.add_operand(pipe);
        result.add_operand(packet_size);
        result.add_operand(packet_alignment);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_reserve_read_pipe_packets(&mut self, result_type: &Inst, execution: &Inst, pipe: &Inst, num_packets: &Inst, packet_size: &Inst, packet_alignment: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_RESERVE_READ_PIPE_PACKETS, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(pipe);
        result.add_operand(num_packets);
        result.add_operand(packet_size);
        result.add_operand(packet_alignment);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_reserve_write_pipe_packets(&mut self, result_type: &Inst, execution: &Inst, pipe: &Inst, num_packets: &Inst, packet_size: &Inst, packet_alignment: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_RESERVE_WRITE_PIPE_PACKETS, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(pipe);
        result.add_operand(num_packets);
        result.add_operand(packet_size);
        result.add_operand(packet_alignment);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_commit_read_pipe(&mut self, execution: &Inst, pipe: &Inst, reserve_id: &Inst, packet_size: &Inst, packet_alignment: &Inst) -> Inst {
        let result = self.new_instruction(Op::GROUP_COMMIT_READ_PIPE, INVALID_ID, None);
        result.add_operand(execution);
        result.add_operand(pipe);
        result.add_operand(reserve_id);
        result.add_operand(packet_size);
        result.add_operand(packet_alignment);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_commit_write_pipe(&mut self, execution: &Inst, pipe: &Inst, reserve_id: &Inst, packet_size: &Inst, packet_alignment: &Inst) -> Inst {
        let result = self.new_instruction(Op::GROUP_COMMIT_WRITE_PIPE, INVALID_ID, None);
        result.add_operand(execution);
        result.add_operand(pipe);
        result.add_operand(reserve_id);
        result.add_operand(packet_size);
        result.add_operand(packet_alignment);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn constant_pipe_storage(&mut self, result_type: &Inst, packet_size: Lit, packet_alignment: Lit, capacity: Lit) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::CONSTANT_PIPE_STORAGE, result_id, Some(result_type));
        result.add_operand(packet_size);
        result.add_operand(packet_alignment);
        result.add_operand(capacity);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn create_pipe_from_pipe_storage(&mut self, result_type: &Inst, pipe_storage: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::CREATE_PIPE_FROM_PIPE_STORAGE, result_id, Some(result_type));
        result.add_operand(pipe_storage);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn read_pipe_blocking_intel(&mut self, result_type: &Inst, packet_size: &Inst, packet_alignment: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::READ_PIPE_BLOCKING_INTEL, result_id, Some(result_type));
        result.add_operand(packet_size);
        result.add_operand(packet_alignment);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn write_pipe_blocking_intel(&mut self, result_type: &Inst, packet_size: &Inst, packet_alignment: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::WRITE_PIPE_BLOCKING_INTEL, result_id, Some(result_type));
        result.add_operand(packet_size);
        result.add_operand(packet_alignment);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_elect(&mut self, result_type: &Inst, execution: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_ELECT, result_id, Some(result_type));
        result.add_operand(execution);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_all(&mut self, result_type: &Inst, execution: &Inst, predicate: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_ALL, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(predicate);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_any(&mut self, result_type: &Inst, execution: &Inst, predicate: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_ANY, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(predicate);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_all_equal(&mut self, result_type: &Inst, execution: &Inst, value: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_ALL_EQUAL, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_broadcast(&mut self, result_type: &Inst, execution: &Inst, value: &Inst, id: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_BROADCAST, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(value);
        result.add_operand(id);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_broadcast_first(&mut self, result_type: &Inst, execution: &Inst, value: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_BROADCAST_FIRST, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_ballot(&mut self, result_type: &Inst, execution: &Inst, predicate: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_BALLOT, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(predicate);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_inverse_ballot(&mut self, result_type: &Inst, execution: &Inst, value: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_INVERSE_BALLOT, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_ballot_bit_extract(&mut self, result_type: &Inst, execution: &Inst, value: &Inst, index: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_BALLOT_BIT_EXTRACT, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(value);
        result.add_operand(index);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_ballot_bit_count(&mut self, result_type: &Inst, execution: &Inst, operation: GroupOperation, value: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_BALLOT_BIT_COUNT, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(operation);
        result.add_operand(value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_ballot_find_lsb(&mut self, result_type: &Inst, execution: &Inst, value: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_BALLOT_FIND_LSB, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_ballot_find_msb(&mut self, result_type: &Inst, execution: &Inst, value: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_BALLOT_FIND_MSB, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_shuffle(&mut self, result_type: &Inst, execution: &Inst, value: &Inst, id: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_SHUFFLE, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(value);
        result.add_operand(id);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_shuffle_xor(&mut self, result_type: &Inst, execution: &Inst, value: &Inst, mask: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_SHUFFLE_XOR, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(value);
        result.add_operand(mask);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_shuffle_up(&mut self, result_type: &Inst, execution: &Inst, value: &Inst, delta: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_SHUFFLE_UP, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(value);
        result.add_operand(delta);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_shuffle_down(&mut self, result_type: &Inst, execution: &Inst, value: &Inst, delta: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_SHUFFLE_DOWN, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(value);
        result.add_operand(delta);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_i_add(&mut self, result_type: &Inst, execution: &Inst, operation: GroupOperation, value: &Inst, cluster_size: Option<&Inst>) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_I_ADD, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(operation);
        result.add_operand(value);
        if let Some(v) = cluster_size {
            result.add_operand(v);
        }
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_f_add(&mut self, result_type: &Inst, execution: &Inst, operation: GroupOperation, value: &Inst, cluster_size: Option<&Inst>) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_F_ADD, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(operation);
        result.add_operand(value);
        if let Some(v) = cluster_size {
            result.add_operand(v);
        }
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_i_mul(&mut self, result_type: &Inst, execution: &Inst, operation: GroupOperation, value: &Inst, cluster_size: Option<&Inst>) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_I_MUL, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(operation);
        result.add_operand(value);
        if let Some(v) = cluster_size {
            result.add_operand(v);
        }
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_f_mul(&mut self, result_type: &Inst, execution: &Inst, operation: GroupOperation, value: &Inst, cluster_size: Option<&Inst>) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_F_MUL, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(operation);
        result.add_operand(value);
        if let Some(v) = cluster_size {
            result.add_operand(v);
        }
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_s_min(&mut self, result_type: &Inst, execution: &Inst, operation: GroupOperation, value: &Inst, cluster_size: Option<&Inst>) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_S_MIN, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(operation);
        result.add_operand(value);
        if let Some(v) = cluster_size {
            result.add_operand(v);
        }
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_u_min(&mut self, result_type: &Inst, execution: &Inst, operation: GroupOperation, value: &Inst, cluster_size: Option<&Inst>) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_U_MIN, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(operation);
        result.add_operand(value);
        if let Some(v) = cluster_size {
            result.add_operand(v);
        }
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_f_min(&mut self, result_type: &Inst, execution: &Inst, operation: GroupOperation, value: &Inst, cluster_size: Option<&Inst>) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_F_MIN, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(operation);
        result.add_operand(value);
        if let Some(v) = cluster_size {
            result.add_operand(v);
        }
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_s_max(&mut self, result_type: &Inst, execution: &Inst, operation: GroupOperation, value: &Inst, cluster_size: Option<&Inst>) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_S_MAX, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(operation);
        result.add_operand(value);
        if let Some(v) = cluster_size {
            result.add_operand(v);
        }
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_u_max(&mut self, result_type: &Inst, execution: &Inst, operation: GroupOperation, value: &Inst, cluster_size: Option<&Inst>) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_U_MAX, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(operation);
        result.add_operand(value);
        if let Some(v) = cluster_size {
            result.add_operand(v);
        }
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_f_max(&mut self, result_type: &Inst, execution: &Inst, operation: GroupOperation, value: &Inst, cluster_size: Option<&Inst>) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_F_MAX, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(operation);
        result.add_operand(value);
        if let Some(v) = cluster_size {
            result.add_operand(v);
        }
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_bitwise_and(&mut self, result_type: &Inst, execution: &Inst, operation: GroupOperation, value: &Inst, cluster_size: Option<&Inst>) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_BITWISE_AND, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(operation);
        result.add_operand(value);
        if let Some(v) = cluster_size {
            result.add_operand(v);
        }
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_bitwise_or(&mut self, result_type: &Inst, execution: &Inst, operation: GroupOperation, value: &Inst, cluster_size: Option<&Inst>) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_BITWISE_OR, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(operation);
        result.add_operand(value);
        if let Some(v) = cluster_size {
            result.add_operand(v);
        }
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_bitwise_xor(&mut self, result_type: &Inst, execution: &Inst, operation: GroupOperation, value: &Inst, cluster_size: Option<&Inst>) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_BITWISE_XOR, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(operation);
        result.add_operand(value);
        if let Some(v) = cluster_size {
            result.add_operand(v);
        }
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_logical_and(&mut self, result_type: &Inst, execution: &Inst, operation: GroupOperation, value: &Inst, cluster_size: Option<&Inst>) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_LOGICAL_AND, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(operation);
        result.add_operand(value);
        if let Some(v) = cluster_size {
            result.add_operand(v);
        }
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_logical_or(&mut self, result_type: &Inst, execution: &Inst, operation: GroupOperation, value: &Inst, cluster_size: Option<&Inst>) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_LOGICAL_OR, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(operation);
        result.add_operand(value);
        if let Some(v) = cluster_size {
            result.add_operand(v);
        }
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_logical_xor(&mut self, result_type: &Inst, execution: &Inst, operation: GroupOperation, value: &Inst, cluster_size: Option<&Inst>) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_LOGICAL_XOR, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(operation);
        result.add_operand(value);
        if let Some(v) = cluster_size {
            result.add_operand(v);
        }
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_quad_broadcast(&mut self, result_type: &Inst, execution: &Inst, value: &Inst, index: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_QUAD_BROADCAST, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(value);
        result.add_operand(index);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_quad_swap(&mut self, result_type: &Inst, execution: &Inst, value: &Inst, direction: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_QUAD_SWAP, result_id, Some(result_type));
        result.add_operand(execution);
        result.add_operand(value);
        result.add_operand(direction);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn group_non_uniform_partition_nv(&mut self, result_type: &Inst, value: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::GROUP_NON_UNIFORM_PARTITION_NV, result_id, Some(result_type));
        result.add_operand(value);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn trace_ray_khr(&mut self, accel: &Inst, ray_flags: &Inst, cull_mask: &Inst, s_bt_offset: &Inst, s_bt_stride: &Inst, miss_index: &Inst, ray_origin: &Inst, ray_tmin: &Inst, ray_direction: &Inst, ray_tmax: &Inst, payload: &Inst) -> Inst {
        let result = self.new_instruction(Op::TRACE_RAY_KHR, INVALID_ID, None);
        result.add_operand(accel);
        result.add_operand(ray_flags);
        result.add_operand(cull_mask);
        result.add_operand(s_bt_offset);
        result.add_operand(s_bt_stride);
        result.add_operand(miss_index);
        result.add_operand(ray_origin);
        result.add_operand(ray_tmin);
        result.add_operand(ray_direction);
        result.add_operand(ray_tmax);
        result.add_operand(payload);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn execute_callable_khr(&mut self, s_bt_index: &Inst, callable_data: &Inst) -> Inst {
        let result = self.new_instruction(Op::EXECUTE_CALLABLE_KHR, INVALID_ID, None);
        result.add_operand(s_bt_index);
        result.add_operand(callable_data);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn convert_u_to_acceleration_structure_khr(&mut self, result_type: &Inst, accel: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::CONVERT_U_TO_ACCELERATION_STRUCTURE_KHR, result_id, Some(result_type));
        result.add_operand(accel);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn ignore_intersection_khr(&mut self, ) -> Inst {
        let result = self.new_instruction(Op::IGNORE_INTERSECTION_KHR, INVALID_ID, None);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn terminate_ray_khr(&mut self, ) -> Inst {
        let result = self.new_instruction(Op::TERMINATE_RAY_KHR, INVALID_ID, None);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn type_ray_query_khr(&mut self, ) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::TYPE_RAY_QUERY_KHR, result_id, None);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn ray_query_initialize_khr(&mut self, ray_query: &Inst, accel: &Inst, ray_flags: &Inst, cull_mask: &Inst, ray_origin: &Inst, ray_t_min: &Inst, ray_direction: &Inst, ray_t_max: &Inst) -> Inst {
        let result = self.new_instruction(Op::RAY_QUERY_INITIALIZE_KHR, INVALID_ID, None);
        result.add_operand(ray_query);
        result.add_operand(accel);
        result.add_operand(ray_flags);
        result.add_operand(cull_mask);
        result.add_operand(ray_origin);
        result.add_operand(ray_t_min);
        result.add_operand(ray_direction);
        result.add_operand(ray_t_max);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn ray_query_terminate_khr(&mut self, ray_query: &Inst) -> Inst {
        let result = self.new_instruction(Op::RAY_QUERY_TERMINATE_KHR, INVALID_ID, None);
        result.add_operand(ray_query);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn ray_query_generate_intersection_khr(&mut self, ray_query: &Inst, hit_t: &Inst) -> Inst {
        let result = self.new_instruction(Op::RAY_QUERY_GENERATE_INTERSECTION_KHR, INVALID_ID, None);
        result.add_operand(ray_query);
        result.add_operand(hit_t);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn ray_query_confirm_intersection_khr(&mut self, ray_query: &Inst) -> Inst {
        let result = self.new_instruction(Op::RAY_QUERY_CONFIRM_INTERSECTION_KHR, INVALID_ID, None);
        result.add_operand(ray_query);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn ray_query_proceed_khr(&mut self, result_type: &Inst, ray_query: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::RAY_QUERY_PROCEED_KHR, result_id, Some(result_type));
        result.add_operand(ray_query);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn ray_query_get_intersection_type_khr(&mut self, result_type: &Inst, ray_query: &Inst, intersection: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::RAY_QUERY_GET_INTERSECTION_TYPE_KHR, result_id, Some(result_type));
        result.add_operand(ray_query);
        result.add_operand(intersection);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn fragment_mask_fetch_amd(&mut self, result_type: &Inst, image: &Inst, coordinate: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::FRAGMENT_MASK_FETCH_AMD, result_id, Some(result_type));
        result.add_operand(image);
        result.add_operand(coordinate);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn fragment_fetch_amd(&mut self, result_type: &Inst, image: &Inst, coordinate: &Inst, fragment_index: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::FRAGMENT_FETCH_AMD, result_id, Some(result_type));
        result.add_operand(image);
        result.add_operand(coordinate);
        result.add_operand(fragment_index);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn read_clock_khr(&mut self, result_type: &Inst, execution: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::READ_CLOCK_KHR, result_id, Some(result_type));
        result.add_operand(execution);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn write_packed_primitive_indices4x8_nv(&mut self, index_offset: &Inst, packed_indices: &Inst) -> Inst {
        let result = self.new_instruction(Op::WRITE_PACKED_PRIMITIVE_INDICES4X8_NV, INVALID_ID, None);
        result.add_operand(index_offset);
        result.add_operand(packed_indices);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn report_intersection_nv(&mut self, result_type: &Inst, hit: &Inst, hit_kind: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::REPORT_INTERSECTION_KHR, result_id, Some(result_type));
        result.add_operand(hit);
        result.add_operand(hit_kind);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn report_intersection_khr(&mut self, result_type: &Inst, hit: &Inst, hit_kind: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::REPORT_INTERSECTION_KHR, result_id, Some(result_type));
        result.add_operand(hit);
        result.add_operand(hit_kind);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn ignore_intersection_nv(&mut self, ) -> Inst {
        let result = self.new_instruction(Op::IGNORE_INTERSECTION_NV, INVALID_ID, None);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn terminate_ray_nv(&mut self, ) -> Inst {
        let result = self.new_instruction(Op::TERMINATE_RAY_NV, INVALID_ID, None);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn trace_nv(&mut self, accel: &Inst, ray_flags: &Inst, cull_mask: &Inst, s_bt_offset: &Inst, s_bt_stride: &Inst, miss_index: &Inst, ray_origin: &Inst, ray_tmin: &Inst, ray_direction: &Inst, ray_tmax: &Inst, payload_id: &Inst) -> Inst {
        let result = self.new_instruction(Op::TRACE_NV, INVALID_ID, None);
        result.add_operand(accel);
        result.add_operand(ray_flags);
        result.add_operand(cull_mask);
        result.add_operand(s_bt_offset);
        result.add_operand(s_bt_stride);
        result.add_operand(miss_index);
        result.add_operand(ray_origin);
        result.add_operand(ray_tmin);
        result.add_operand(ray_direction);
        result.add_operand(ray_tmax);
        result.add_operand(payload_id);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn type_acceleration_structure_nv(&mut self, ) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::TYPE_ACCELERATION_STRUCTURE_KHR, result_id, None);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn type_acceleration_structure_khr(&mut self, ) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::TYPE_ACCELERATION_STRUCTURE_KHR, result_id, None);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn execute_callable_nv(&mut self, s_bt_index: &Inst, callable_data_id: &Inst) -> Inst {
        let result = self.new_instruction(Op::EXECUTE_CALLABLE_NV, INVALID_ID, None);
        result.add_operand(s_bt_index);
        result.add_operand(callable_data_id);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn type_cooperative_matrix_nv(&mut self, component_type: &Inst, execution: &Inst, rows: &Inst, columns: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::TYPE_COOPERATIVE_MATRIX_NV, result_id, None);
        result.add_operand(component_type);
        result.add_operand(execution);
        result.add_operand(rows);
        result.add_operand(columns);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn cooperative_matrix_load_nv(&mut self, result_type: &Inst, pointer: &Inst, stride: &Inst, column_major: &Inst, memory_access: Option<MemoryAccessMask>) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::COOPERATIVE_MATRIX_LOAD_NV, result_id, Some(result_type));
        result.add_operand(pointer);
        result.add_operand(stride);
        result.add_operand(column_major);
        if let Some(v) = memory_access {
            result.add_operand(v);
        }
        self.add_to_function_definitions(&result);
        result
    }

    pub fn cooperative_matrix_store_nv(&mut self, pointer: &Inst, obj: &Inst, stride: &Inst, column_major: &Inst, memory_access: Option<MemoryAccessMask>) -> Inst {
        let result = self.new_instruction(Op::COOPERATIVE_MATRIX_STORE_NV, INVALID_ID, None);
        result.add_operand(pointer);
        result.add_operand(obj);
        result.add_operand(stride);
        result.add_operand(column_major);
        if let Some(v) = memory_access {
            result.add_operand(v);
        }
        self.add_to_function_definitions(&result);
        result
    }

    pub fn cooperative_matrix_mul_add_nv(&mut self, result_type: &Inst, a: &Inst, b: &Inst, c: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::COOPERATIVE_MATRIX_MUL_ADD_NV, result_id, Some(result_type));
        result.add_operand(a);
        result.add_operand(b);
        result.add_operand(c);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn cooperative_matrix_length_nv(&mut self, result_type: &Inst, type_: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::COOPERATIVE_MATRIX_LENGTH_NV, result_id, Some(result_type));
        result.add_operand(type_);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn begin_invocation_interlock_ext(&mut self, ) -> Inst {
        let result = self.new_instruction(Op::BEGIN_INVOCATION_INTERLOCK_EXT, INVALID_ID, None);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn end_invocation_interlock_ext(&mut self, ) -> Inst {
        let result = self.new_instruction(Op::END_INVOCATION_INTERLOCK_EXT, INVALID_ID, None);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn demote_to_helper_invocation_ext(&mut self, ) -> Inst {
        let result = self.new_instruction(Op::DEMOTE_TO_HELPER_INVOCATION_EXT, INVALID_ID, None);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn is_helper_invocation_ext(&mut self, result_type: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::IS_HELPER_INVOCATION_EXT, result_id, Some(result_type));
        self.add_to_function_definitions(&result);
        result
    }

    pub fn u_count_leading_zeros_intel(&mut self, result_type: &Inst, operand: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::U_COUNT_LEADING_ZEROS_INTEL, result_id, Some(result_type));
        result.add_operand(operand);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn u_count_trailing_zeros_intel(&mut self, result_type: &Inst, operand: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::U_COUNT_TRAILING_ZEROS_INTEL, result_id, Some(result_type));
        result.add_operand(operand);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn abs_i_sub_intel(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::ABS_I_SUB_INTEL, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn abs_u_sub_intel(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::ABS_U_SUB_INTEL, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn i_add_sat_intel(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::I_ADD_SAT_INTEL, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn u_add_sat_intel(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::U_ADD_SAT_INTEL, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn i_average_intel(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::I_AVERAGE_INTEL, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn u_average_intel(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::U_AVERAGE_INTEL, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn i_average_rounded_intel(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::I_AVERAGE_ROUNDED_INTEL, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn u_average_rounded_intel(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::U_AVERAGE_ROUNDED_INTEL, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn i_sub_sat_intel(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::I_SUB_SAT_INTEL, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn u_sub_sat_intel(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::U_SUB_SAT_INTEL, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn i_mul32x16_intel(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::I_MUL32X16_INTEL, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn u_mul32x16_intel(&mut self, result_type: &Inst, operand1: &Inst, operand2: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::U_MUL32X16_INTEL, result_id, Some(result_type));
        result.add_operand(operand1);
        result.add_operand(operand2);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn loop_control_intel(&mut self, loop_control_parameters: &[Operand]) -> Inst {
        let result = self.new_instruction(Op::LOOP_CONTROL_INTEL, INVALID_ID, None);
        result.add_operands(loop_control_parameters);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn fpga_reg_intel(&mut self, result_type: &Inst, result_obj: &Inst, input: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::FPGA_REG_INTEL, result_id, Some(result_type));
        result.add_operand(result_obj);
        result.add_operand(input);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn ray_query_get_ray_t_min_khr(&mut self, result_type: &Inst, ray_query: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::RAY_QUERY_GET_RAY_T_MIN_KHR, result_id, Some(result_type));
        result.add_operand(ray_query);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn ray_query_get_ray_flags_khr(&mut self, result_type: &Inst, ray_query: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::RAY_QUERY_GET_RAY_FLAGS_KHR, result_id, Some(result_type));
        result.add_operand(ray_query);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn ray_query_get_intersection_tkhr(&mut self, result_type: &Inst, ray_query: &Inst, intersection: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::RAY_QUERY_GET_INTERSECTION_TKHR, result_id, Some(result_type));
        result.add_operand(ray_query);
        result.add_operand(intersection);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn ray_query_get_intersection_instance_custom_index_khr(&mut self, result_type: &Inst, ray_query: &Inst, intersection: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::RAY_QUERY_GET_INTERSECTION_INSTANCE_CUSTOM_INDEX_KHR, result_id, Some(result_type));
        result.add_operand(ray_query);
        result.add_operand(intersection);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn ray_query_get_intersection_instance_id_khr(&mut self, result_type: &Inst, ray_query: &Inst, intersection: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::RAY_QUERY_GET_INTERSECTION_INSTANCE_ID_KHR, result_id, Some(result_type));
        result.add_operand(ray_query);
        result.add_operand(intersection);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn ray_query_get_intersection_instance_shader_binding_table_record_offset_khr(&mut self, result_type: &Inst, ray_query: &Inst, intersection: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::RAY_QUERY_GET_INTERSECTION_INSTANCE_SHADER_BINDING_TABLE_RECORD_OFFSET_KHR, result_id, Some(result_type));
        result.add_operand(ray_query);
        result.add_operand(intersection);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn ray_query_get_intersection_geometry_index_khr(&mut self, result_type: &Inst, ray_query: &Inst, intersection: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::RAY_QUERY_GET_INTERSECTION_GEOMETRY_INDEX_KHR, result_id, Some(result_type));
        result.add_operand(ray_query);
        result.add_operand(intersection);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn ray_query_get_intersection_primitive_index_khr(&mut self, result_type: &Inst, ray_query: &Inst, intersection: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::RAY_QUERY_GET_INTERSECTION_PRIMITIVE_INDEX_KHR, result_id, Some(result_type));
        result.add_operand(ray_query);
        result.add_operand(intersection);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn ray_query_get_intersection_barycentrics_khr(&mut self, result_type: &Inst, ray_query: &Inst, intersection: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::RAY_QUERY_GET_INTERSECTION_BARYCENTRICS_KHR, result_id, Some(result_type));
        result.add_operand(ray_query);
        result.add_operand(intersection);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn ray_query_get_intersection_front_face_khr(&mut self, result_type: &Inst, ray_query: &Inst, intersection: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::RAY_QUERY_GET_INTERSECTION_FRONT_FACE_KHR, result_id, Some(result_type));
        result.add_operand(ray_query);
        result.add_operand(intersection);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn ray_query_get_intersection_candidate_aabb_opaque_khr(&mut self, result_type: &Inst, ray_query: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::RAY_QUERY_GET_INTERSECTION_CANDIDATE_AABB_OPAQUE_KHR, result_id, Some(result_type));
        result.add_operand(ray_query);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn ray_query_get_intersection_object_ray_direction_khr(&mut self, result_type: &Inst, ray_query: &Inst, intersection: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::RAY_QUERY_GET_INTERSECTION_OBJECT_RAY_DIRECTION_KHR, result_id, Some(result_type));
        result.add_operand(ray_query);
        result.add_operand(intersection);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn ray_query_get_intersection_object_ray_origin_khr(&mut self, result_type: &Inst, ray_query: &Inst, intersection: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::RAY_QUERY_GET_INTERSECTION_OBJECT_RAY_ORIGIN_KHR, result_id, Some(result_type));
        result.add_operand(ray_query);
        result.add_operand(intersection);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn ray_query_get_world_ray_direction_khr(&mut self, result_type: &Inst, ray_query: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::RAY_QUERY_GET_WORLD_RAY_DIRECTION_KHR, result_id, Some(result_type));
        result.add_operand(ray_query);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn ray_query_get_world_ray_origin_khr(&mut self, result_type: &Inst, ray_query: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::RAY_QUERY_GET_WORLD_RAY_ORIGIN_KHR, result_id, Some(result_type));
        result.add_operand(ray_query);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn ray_query_get_intersection_object_to_world_khr(&mut self, result_type: &Inst, ray_query: &Inst, intersection: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::RAY_QUERY_GET_INTERSECTION_OBJECT_TO_WORLD_KHR, result_id, Some(result_type));
        result.add_operand(ray_query);
        result.add_operand(intersection);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn ray_query_get_intersection_world_to_object_khr(&mut self, result_type: &Inst, ray_query: &Inst, intersection: &Inst) -> Inst {
        let result_id = self.new_id();
        let result = self.new_instruction(Op::RAY_QUERY_GET_INTERSECTION_WORLD_TO_OBJECT_KHR, result_id, Some(result_type));
        result.add_operand(ray_query);
        result.add_operand(intersection);
        self.add_to_function_definitions(&result);
        result
    }

    pub fn glsl_round(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 1, &[Operand::from(x)])
    }

    pub fn glsl_round_even(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 2, &[Operand::from(x)])
    }

    pub fn glsl_trunc(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 3, &[Operand::from(x)])
    }

    pub fn glsl_f_abs(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 4, &[Operand::from(x)])
    }

    pub fn glsl_s_abs(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 5, &[Operand::from(x)])
    }

    pub fn glsl_f_sign(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 6, &[Operand::from(x)])
    }

    pub fn glsl_s_sign(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 7, &[Operand::from(x)])
    }

    pub fn glsl_floor(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 8, &[Operand::from(x)])
    }

    pub fn glsl_ceil(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 9, &[Operand::from(x)])
    }

    pub fn glsl_fract(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 10, &[Operand::from(x)])
    }

    pub fn glsl_radians(&mut self, result_type: &Inst, degrees: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 11, &[Operand::from(degrees)])
    }

    pub fn glsl_degrees(&mut self, result_type: &Inst, radians: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 12, &[Operand::from(radians)])
    }

    pub fn glsl_sin(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 13, &[Operand::from(x)])
    }

    pub fn glsl_cos(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 14, &[Operand::from(x)])
    }

    pub fn glsl_tan(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 15, &[Operand::from(x)])
    }

    pub fn glsl_asin(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 16, &[Operand::from(x)])
    }

    pub fn glsl_acos(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 17, &[Operand::from(x)])
    }

    pub fn glsl_atan(&mut self, result_type: &Inst, y_over_x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 18, &[Operand::from(y_over_x)])
    }

    pub fn glsl_sinh(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 19, &[Operand::from(x)])
    }

    pub fn glsl_cosh(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 20, &[Operand::from(x)])
    }

    pub fn glsl_tanh(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 21, &[Operand::from(x)])
    }

    pub fn glsl_asinh(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 22, &[Operand::from(x)])
    }

    pub fn glsl_acosh(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 23, &[Operand::from(x)])
    }

    pub fn glsl_atanh(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 24, &[Operand::from(x)])
    }

    pub fn glsl_atan2(&mut self, result_type: &Inst, y: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 25, &[Operand::from(y), Operand::from(x)])
    }

    pub fn glsl_pow(&mut self, result_type: &Inst, x: &Inst, y: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 26, &[Operand::from(x), Operand::from(y)])
    }

    pub fn glsl_exp(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 27, &[Operand::from(x)])
    }

    pub fn glsl_log(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 28, &[Operand::from(x)])
    }

    pub fn glsl_exp2(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 29, &[Operand::from(x)])
    }

    pub fn glsl_log2(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 30, &[Operand::from(x)])
    }

    pub fn glsl_sqrt(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 31, &[Operand::from(x)])
    }

    pub fn glsl_inverse_sqrt(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 32, &[Operand::from(x)])
    }

    pub fn glsl_determinant(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 33, &[Operand::from(x)])
    }

    pub fn glsl_matrix_inverse(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 34, &[Operand::from(x)])
    }

    pub fn glsl_modf(&mut self, result_type: &Inst, x: &Inst, i: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 35, &[Operand::from(x), Operand::from(i)])
    }

    pub fn glsl_modf_struct(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 36, &[Operand::from(x)])
    }

    pub fn glsl_f_min(&mut self, result_type: &Inst, x: &Inst, y: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 37, &[Operand::from(x), Operand::from(y)])
    }

    pub fn glsl_u_min(&mut self, result_type: &Inst, x: &Inst, y: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 38, &[Operand::from(x), Operand::from(y)])
    }

    pub fn glsl_s_min(&mut self, result_type: &Inst, x: &Inst, y: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 39, &[Operand::from(x), Operand::from(y)])
    }

    pub fn glsl_f_max(&mut self, result_type: &Inst, x: &Inst, y: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 40, &[Operand::from(x), Operand::from(y)])
    }

    pub fn glsl_u_max(&mut self, result_type: &Inst, x: &Inst, y: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 41, &[Operand::from(x), Operand::from(y)])
    }

    pub fn glsl_s_max(&mut self, result_type: &Inst, x: &Inst, y: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 42, &[Operand::from(x), Operand::from(y)])
    }

    pub fn glsl_f_clamp(&mut self, result_type: &Inst, x: &Inst, min_val: &Inst, max_val: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 43, &[Operand::from(x), Operand::from(min_val), Operand::from(max_val)])
    }

    pub fn glsl_u_clamp(&mut self, result_type: &Inst, x: &Inst, min_val: &Inst, max_val: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 44, &[Operand::from(x), Operand::from(min_val), Operand::from(max_val)])
    }

    pub fn glsl_s_clamp(&mut self, result_type: &Inst, x: &Inst, min_val: &Inst, max_val: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 45, &[Operand::from(x), Operand::from(min_val), Operand::from(max_val)])
    }

    pub fn glsl_f_mix(&mut self, result_type: &Inst, x: &Inst, y: &Inst, a: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 46, &[Operand::from(x), Operand::from(y), Operand::from(a)])
    }

    pub fn glsl_i_mix(&mut self, result_type: &Inst, x: &Inst, y: &Inst, a: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 47, &[Operand::from(x), Operand::from(y), Operand::from(a)])
    }

    pub fn glsl_step(&mut self, result_type: &Inst, edge: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 48, &[Operand::from(edge), Operand::from(x)])
    }

    pub fn glsl_smooth_step(&mut self, result_type: &Inst, edge0: &Inst, edge1: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 49, &[Operand::from(edge0), Operand::from(edge1), Operand::from(x)])
    }

    pub fn glsl_fma(&mut self, result_type: &Inst, a: &Inst, b: &Inst, c: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 50, &[Operand::from(a), Operand::from(b), Operand::from(c)])
    }

    pub fn glsl_frexp(&mut self, result_type: &Inst, x: &Inst, exp: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 51, &[Operand::from(x), Operand::from(exp)])
    }

    pub fn glsl_frexp_struct(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 52, &[Operand::from(x)])
    }

    pub fn glsl_ldexp(&mut self, result_type: &Inst, x: &Inst, exp: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 53, &[Operand::from(x), Operand::from(exp)])
    }

    pub fn glsl_pack_snorm4x8(&mut self, result_type: &Inst, v: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 54, &[Operand::from(v)])
    }

    pub fn glsl_pack_unorm4x8(&mut self, result_type: &Inst, v: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 55, &[Operand::from(v)])
    }

    pub fn glsl_pack_snorm2x16(&mut self, result_type: &Inst, v: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 56, &[Operand::from(v)])
    }

    pub fn glsl_pack_unorm2x16(&mut self, result_type: &Inst, v: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 57, &[Operand::from(v)])
    }

    pub fn glsl_pack_half2x16(&mut self, result_type: &Inst, v: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 58, &[Operand::from(v)])
    }

    pub fn glsl_pack_double2x32(&mut self, result_type: &Inst, v: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 59, &[Operand::from(v)])
    }

    pub fn glsl_unpack_snorm2x16(&mut self, result_type: &Inst, p: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 60, &[Operand::from(p)])
    }

    pub fn glsl_unpack_unorm2x16(&mut self, result_type: &Inst, p: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 61, &[Operand::from(p)])
    }

    pub fn glsl_unpack_half2x16(&mut self, result_type: &Inst, v: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 62, &[Operand::from(v)])
    }

    pub fn glsl_unpack_snorm4x8(&mut self, result_type: &Inst, p: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 63, &[Operand::from(p)])
    }

    pub fn glsl_unpack_unorm4x8(&mut self, result_type: &Inst, p: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 64, &[Operand::from(p)])
    }

    pub fn glsl_unpack_double2x32(&mut self, result_type: &Inst, v: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 65, &[Operand::from(v)])
    }

    pub fn glsl_length(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 66, &[Operand::from(x)])
    }

    pub fn glsl_distance(&mut self, result_type: &Inst, p0: &Inst, p1: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 67, &[Operand::from(p0), Operand::from(p1)])
    }

    pub fn glsl_cross(&mut self, result_type: &Inst, x: &Inst, y: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 68, &[Operand::from(x), Operand::from(y)])
    }

    pub fn glsl_normalize(&mut self, result_type: &Inst, x: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 69, &[Operand::from(x)])
    }

    pub fn glsl_face_forward(&mut self, result_type: &Inst, n: &Inst, i: &Inst, nref: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 70, &[Operand::from(n), Operand::from(i), Operand::from(nref)])
    }

    pub fn glsl_reflect(&mut self, result_type: &Inst, i: &Inst, n: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 71, &[Operand::from(i), Operand::from(n)])
    }

    pub fn glsl_refract(&mut self, result_type: &Inst, i: &Inst, n: &Inst, eta: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 72, &[Operand::from(i), Operand::from(n), Operand::from(eta)])
    }

    pub fn glsl_find_i_lsb(&mut self, result_type: &Inst, value: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 73, &[Operand::from(value)])
    }

    pub fn glsl_find_s_msb(&mut self, result_type: &Inst, value: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 74, &[Operand::from(value)])
    }

    pub fn glsl_find_u_msb(&mut self, result_type: &Inst, value: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 75, &[Operand::from(value)])
    }

    pub fn glsl_interpolate_at_centroid(&mut self, result_type: &Inst, interpolant: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 76, &[Operand::from(interpolant)])
    }

    pub fn glsl_interpolate_at_sample(&mut self, result_type: &Inst, interpolant: &Inst, sample: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 77, &[Operand::from(interpolant), Operand::from(sample)])
    }

    pub fn glsl_interpolate_at_offset(&mut self, result_type: &Inst, interpolant: &Inst, offset: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 78, &[Operand::from(interpolant), Operand::from(offset)])
    }

    pub fn glsl_n_min(&mut self, result_type: &Inst, x: &Inst, y: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 79, &[Operand::from(x), Operand::from(y)])
    }

    pub fn glsl_n_max(&mut self, result_type: &Inst, x: &Inst, y: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 80, &[Operand::from(x), Operand::from(y)])
    }

    pub fn glsl_n_clamp(&mut self, result_type: &Inst, x: &Inst, min_val: &Inst, max_val: &Inst) -> Inst {
        let set = self.add_ext_inst_import("GLSL.std.450");
        self.ext_inst(result_type, &set, 81, &[Operand::from(x), Operand::from(min_val), Operand::from(max_val)])
    }
}
