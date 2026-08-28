//! Local DXBC instruction adapter for `cfglib`.

use alloc::borrow::Cow;

use cfglib::{FlowControl, FlowEffect};
use d3dasm::dxbc::shex::{Instruction, Opcode};

/// A decoded shader instruction with the control-flow classification needed
/// by [`cfglib::CfgBuilder`].
#[derive(Debug, Clone)]
pub(super) struct Sm4Instruction(Instruction);

impl Sm4Instruction {
    pub(super) const fn new(instruction: Instruction) -> Self {
        Self(instruction)
    }

    pub(super) const fn instruction(&self) -> &Instruction {
        &self.0
    }
}

impl FlowControl for Sm4Instruction {
    fn flow_effect(&self) -> FlowEffect {
        match self.0.opcode {
            Opcode::If => FlowEffect::ConditionalOpen,
            Opcode::Else => FlowEffect::ConditionalAlternate,
            Opcode::EndIf => FlowEffect::ConditionalClose,
            Opcode::Switch => FlowEffect::SwitchOpen,
            Opcode::Case | Opcode::Default => FlowEffect::SwitchCase,
            Opcode::EndSwitch => FlowEffect::SwitchClose,
            Opcode::Loop => FlowEffect::LoopOpen,
            Opcode::EndLoop => FlowEffect::LoopClose,
            Opcode::Break => FlowEffect::Break,
            Opcode::Breakc => FlowEffect::ConditionalBreak,
            Opcode::Continue => FlowEffect::Continue,
            Opcode::Continuec => FlowEffect::ConditionalContinue,
            Opcode::Ret => FlowEffect::Return,
            Opcode::Retc => FlowEffect::ConditionalReturn,
            Opcode::Call | Opcode::InterfaceCall => FlowEffect::Call,
            Opcode::Callc => FlowEffect::ConditionalCall,
            Opcode::Abort => FlowEffect::Terminate,
            Opcode::Label => FlowEffect::Label,
            _ => FlowEffect::Fallthrough,
        }
    }
}

impl cfglib::DisplayInstr for Sm4Instruction {
    fn mnemonic(&self) -> Cow<'_, str> {
        Cow::Borrowed(self.0.opcode.name())
    }
}
