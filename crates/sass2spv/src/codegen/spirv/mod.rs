pub mod context;
pub mod declarations;
pub mod instructions;

pub use context::CodeGenContext;
pub use declarations::Declarations;
pub use instructions::Instructions;

use crate::ir::basic_block::BasicBlock;
use crate::ir::cfg::ControlFlowGraph;
use crate::ir::operation::Operation;
use crate::ir::storage_kind::AggregateType;
use crate::options::{ShaderStage, TranslationOptions};
use crate::structured_ir::ast::{AstBlockType, AstNodeKind};
use crate::structured_ir::program::StructuredProgramContext;
use crate::structured_ir::visitor::{AstBlockVisitor, AstVisitEvent};
use byteorder::{ByteOrder, LittleEndian};
use rspirv::binary::Assemble;
use rspirv::spirv;

pub struct SpirvGenerator<'a> {
    options: &'a TranslationOptions,
    stage: ShaderStage,
}

impl<'a> SpirvGenerator<'a> {
    pub fn new(options: &'a TranslationOptions, stage: ShaderStage) -> Self {
        Self { options, stage }
    }

    pub fn generate(&self, operations: &[Operation]) -> Vec<u8> {
        let cfg = ControlFlowGraph::create(operations);
        let prog_ctx = StructuredProgramContext::new(self.stage, self.options.flags.contains(crate::options::TranslationFlags::DEBUG_MODE));
        let (arena, info) = prog_ctx.make_structured_program(&cfg);

        let mut context = CodeGenContext::new(self.stage, self.options.api);
        Declarations::declare_all(&mut context, &info);

        if let Some(main_func) = info.functions.first() {
            let func_type = context.builder.type_function(context.void_type, vec![]);
            let spv_func = context
                .builder
                .begin_function(context.void_type, None, spirv::FunctionControl::NONE, func_type)
                .unwrap();

            context.enter_block(main_func.main_block);
            Declarations::declare_locals(&mut context, &arena, main_func);

            let visitor = AstBlockVisitor::new(main_func.main_block);
            visitor.traverse(&arena, |event| {
                match event {
                    AstVisitEvent::BlockEntered(block_id) => {
                        let parent = arena.nodes[block_id.0].parent.expect("Child block must have parent");
                        let b_type = arena.get_block(block_id).block_type;

                        if b_type == AstBlockType::If {
                            let if_true = context.get_next_label(block_id);
                            let if_false = if let Some(next) = arena.next(block_id) {
                                if arena.get_block(next).block_type == AstBlockType::Else {
                                    context.get_next_label(next)
                                } else {
                                    context.get_next_label(parent)
                                }
                            } else {
                                context.get_next_label(parent)
                            };

                            let cond_node = arena.get_block(block_id).condition.unwrap();
                            let cond_val = context.get_value(&arena, cond_node, AggregateType::BOOL);
                            let merge_label = context.get_next_label(parent);

                            let _ = context.builder.selection_merge(merge_label, spirv::SelectionControl::NONE);
                            let _ = context.builder.branch_conditional(cond_val, if_true, if_false, vec![]);
                            context.enter_block(block_id);
                        } else if b_type == AstBlockType::DoWhile {
                            let continue_target = context.builder.id();
                            let loop_body = context.get_first_label(block_id);
                            let merge_label = context.get_next_label(parent);
                            let loop_target = context.new_block();

                            context.loop_targets.insert(block_id, (loop_target, continue_target));
                            let _ = context.builder.loop_merge(merge_label, continue_target, spirv::LoopControl::NONE, vec![]);
                            let _ = context.builder.branch(loop_body);
                            context.enter_block(block_id);
                        } else {
                            // Else or ElseIf
                            context.enter_block(block_id);
                        }
                    }

                    AstVisitEvent::BlockLeft(block_id) => {
                        let parent = match arena.nodes[block_id.0].parent {
                            Some(p) => p,
                            None => return,
                        };
                        let b_type = arena.get_block(block_id).block_type;

                        if b_type == AstBlockType::DoWhile {
                            let (loop_target, continue_target) = context.loop_targets[&block_id];
                            let _ = context.builder.branch(continue_target);
                            let _ = context.builder.begin_block(Some(continue_target));

                            let cond_node = arena.get_block(block_id).condition.unwrap();
                            let cond_val = context.get_value(&arena, cond_node, AggregateType::BOOL);
                            let merge_label = context.get_next_label(parent);
                            let _ = context.builder.branch_conditional(cond_val, loop_target, merge_label, vec![]);
                        } else {
                            let last_is_cf = arena.get_block(block_id).last.map(|last_id| {
                                if let AstNodeKind::Operation(op) = &arena.nodes[last_id.0].kind {
                                    BasicBlock::is_control_flow_inst(op.inst)
                                } else {
                                    false
                                }
                            }).unwrap_or(false);

                            if !last_is_cf {
                                let merge_label = context.get_next_label(parent);
                                let _ = context.builder.branch(merge_label);
                            }
                        }

                        let has_else = arena.next(block_id).map(|next_id| {
                            if let AstNodeKind::Block(b) = &arena.nodes[next_id.0].kind {
                                b.block_type == AstBlockType::Else || b.block_type == AstBlockType::ElseIf
                            } else {
                                false
                            }
                        }).unwrap_or(false);

                        if !has_else {
                            context.enter_block(parent);
                        }
                    }

                    AstVisitEvent::Node(node_id) => {
                        match &arena.nodes[node_id.0].kind {
                            AstNodeKind::Assignment(asg) => {
                                let val = match &arena.nodes[asg.source.0].kind {
                                    AstNodeKind::Operation(op) => {
                                        Instructions::generate(&mut context, &arena, op)
                                    }
                                    _ => Some(context.get_value(&arena, asg.source, AggregateType::FP32)),
                                };

                                if let Some(v) = val {
                                    if let Some(&ptr) = context.locals.get(&asg.destination) {
                                        let _ = context.builder.store(ptr, v, None, vec![]);
                                    }
                                }
                            }
                            AstNodeKind::Operation(op) => {
                                Instructions::generate(&mut context, &arena, op);
                            }
                            _ => {}
                        }
                    }
                }
            });

            // Ensure last block has a terminator
            let _ = context.builder.ret();
            let _ = context.builder.end_function();

            let exec_model = match self.stage {
                ShaderStage::Compute => spirv::ExecutionModel::GLCompute,
                ShaderStage::Fragment => spirv::ExecutionModel::Fragment,
                _ => spirv::ExecutionModel::Vertex,
            };

            context.builder.entry_point(exec_model, spv_func, "main", context.main_interface.clone());

            if self.stage == ShaderStage::Fragment {
                context.builder.execution_mode(spv_func, spirv::ExecutionMode::OriginUpperLeft, vec![]);
            }
        }

        let words = context.builder.module().assemble();
        let mut bytes = vec![0u8; words.len() * 4];
        for (i, word) in words.iter().enumerate() {
            LittleEndian::write_u32(&mut bytes[i * 4..(i + 1) * 4], *word);
        }
        bytes
    }
}
