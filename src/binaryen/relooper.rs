use std::ops;
use std::ptr;
use super::sys;
use super::builder;
use super::builder::ModuleOwned;

pub struct Relooper {
    relooper: sys::RelooperRef,
    blocks: Vec<Block>,
}

impl Relooper {
    pub fn new() -> Relooper {
        let relooper = unsafe { sys::RelooperCreate() };
        Relooper {
            relooper: relooper,
            blocks: Vec::new(),
        }
    }

    pub fn num_blocks(&self) -> usize {
        self.blocks.len()
    }

    pub fn add_block(&mut self, body: builder::Expression) -> usize {
        self.add_block_raw(body.into())
    }

    pub fn add_block_raw(&mut self, body: sys::BinaryenExpressionRef) -> usize {
        let block = unsafe { sys::RelooperAddBlock(self.relooper, body) };
        self.push_block_raw(block, BlockKind::Basic);
        self.blocks.len() - 1
    }

    pub fn add_block_raw_with_switch(&mut self,
                                     cond: sys::BinaryenExpressionRef,
                                     body: sys::BinaryenExpressionRef)
                                     -> &Block {
        let block = unsafe { sys::RelooperAddBlockWithSwitch(self.relooper, body, cond) };
        self.push_block_raw(block, BlockKind::Switch)
    }

    fn push_block_raw(&mut self, block: sys::RelooperBlockRef, kind: BlockKind) -> &Block {
        let block = Block {
            block: block,
            kind: kind,
        };
        self.blocks.push(block);
        &self.blocks[self.blocks.len() - 1]
    }

    pub fn render(self, func: &mut builder::Fn, start: usize) -> sys::BinaryenExpressionRef {
        let local = {
            let local = func.create_local(builder::ReprType::Int32);
            local.index().into()
        };
        let module = func.module().module;
        unsafe {
            sys::RelooperRenderAndDispose(self.relooper, self.blocks[start].block, local, module)
        }
    }
}

impl ops::Index<usize> for Relooper {
    type Output = Block;
    fn index(&self, index: usize) -> &Self::Output {
        &self.blocks[index]
    }
}

#[derive(Eq, PartialEq)]
pub enum BlockKind {
    Basic,
    Switch,
}

pub struct Block {
    block: sys::RelooperBlockRef,
    kind: BlockKind,
}

impl Block {
    pub fn add_goto(&self, to: &Block) {
        assert!(self.kind == BlockKind::Basic);
        unsafe {
            sys::RelooperAddBranch(self.block,
                                   to.block,
                                   sys::BinaryenExpressionRef(ptr::null_mut()),
                                   sys::BinaryenExpressionRef(ptr::null_mut()))
        }
    }

    pub fn add_cond_branch_raw(&self, cond: sys::BinaryenExpressionRef, target: &Block) {
        assert!(self.kind == BlockKind::Basic);
        unsafe {
            sys::RelooperAddBranch(self.block,
                                   target.block,
                                   cond,
                                   sys::BinaryenExpressionRef(ptr::null_mut()))
        }
    }

    pub fn add_switch_case(&self, key: u32, target: &Block) {
        assert!(self.kind == BlockKind::Switch);
        unsafe {
            sys::RelooperAddBranchForSwitch(self.block,
                                            target.block,
                                            &key.into(),
                                            1u32.into(),
                                            sys::BinaryenExpressionRef(ptr::null_mut()));
        }
    }

    pub fn add_switch_default(&self, target: &Block) {
        assert!(self.kind == BlockKind::Switch);
        unsafe {
            sys::RelooperAddBranchForSwitch(self.block,
                                            target.block,
                                            ptr::null_mut(),
                                            0u32.into(),
                                            sys::BinaryenExpressionRef(ptr::null_mut()));
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use super::super::builder;
    use super::super::builder::ExpressionBuilder;

    #[test]
    fn create_relooper() {
        let _ = Relooper::new();
    }

    #[test]
    fn add_block() {
        let mut m = builder::Module::new();
        let mut r = Relooper::new();
        let _ = r.add_block_raw(m.unreachable().into());
    }

    #[test]
    fn add_two_blocks() {
        let mut m = builder::Module::new();
        let mut r = Relooper::new();
        let _ = r.add_block_raw(m.unreachable().into());
        let _ = r.add_block_raw(m.unreachable().into());
    }
}
