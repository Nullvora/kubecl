use std::fmt::Display;

use super::{Component, Dialect, Elem, Variable};

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub enum FragmentIdent<D: Dialect> {
    A,
    B,
    Accumulator,
    _Dialect(std::marker::PhantomData<D>),
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub enum FragmentLayout<D: Dialect> {
    ColMajor,
    RowMajor,
    _Dialect(std::marker::PhantomData<D>),
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub struct Fragment<D: Dialect> {
    pub ident: FragmentIdent<D>,
    pub m: u8,
    pub n: u8,
    pub k: u8,
    pub elem: Elem<D>,
    pub layout: Option<FragmentLayout<D>>,
}

/// Warp Matrix-Multiply and Accumulate Instruction.
#[derive(Debug, Clone, Copy)]
pub enum WmmaInstruction<D: Dialect> {
    /// Fill the fragment with the value.
    Fill {
        frag: Variable<D>,
        value: Variable<D>,
    },
    /// Load the value into the fragment given the stride.
    Load {
        frag: Variable<D>,
        value: Variable<D>,
        stride: Variable<D>,
        layout: Option<FragmentLayout<D>>,
    },
    /// Executes D=A*B+C;
    ///
    /// For implementing a matmul, `D=C` : `C+=A*B`
    Execute {
        frag_a: Variable<D>,
        frag_b: Variable<D>,
        frag_c: Variable<D>,
        frag_d: Variable<D>,
    },
    /// Store the fragment in an output variable following the stride and the layout.
    Store {
        output: Variable<D>,
        frag: Variable<D>,
        stride: Variable<D>,
        layout: FragmentLayout<D>,
    },
}

impl<D: Dialect> Display for FragmentLayout<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let namespace = D::mma_namespace();
        match self {
            FragmentLayout::ColMajor => f.write_str(format!("{namespace}::col_major").as_str()),
            FragmentLayout::RowMajor => f.write_str(format!("{namespace}::row_major").as_str()),
            FragmentLayout::_Dialect(_) => Ok(()),
        }
    }
}

impl<D: Dialect> Display for FragmentIdent<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let namespace = D::mma_namespace();
        match self {
            FragmentIdent::A => write!(f, "{namespace}::matrix_a"),
            FragmentIdent::B => write!(f, "{namespace}::matrix_b"),
            FragmentIdent::Accumulator => write!(f, "{namespace}::accumulator"),
            FragmentIdent::_Dialect(_) => Ok(()),
        }
    }
}

impl<D: Dialect> Display for Fragment<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let namespace = D::mma_namespace();
        let elem = match self.elem {
            Elem::TF32 => format!("{namespace}::precision::tf32"),
            elem => format!("{elem}"),
        };
        match self.layout {
            Some(layout) => write!(
                f,
                "{namespace}::fragment<{}, {}, {}, {}, {}, {}>",
                self.ident, self.m, self.n, self.k, elem, layout
            ),
            None => write!(
                f,
                "{namespace}::fragment<{}, {}, {}, {}, {}>",
                self.ident, self.m, self.n, self.k, elem,
            ),
        }
    }
}

impl<D: Dialect> Display for WmmaInstruction<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let namespace = D::mma_namespace();
        match self {
            WmmaInstruction::Fill { frag, value } => {
                writeln!(f, "{namespace}::fill_fragment({frag}, {value});")
            }
            WmmaInstruction::Load {
                frag,
                value,
                stride,
                layout: None,
            } => {
                let item = value.item();
                if item.vectorization > 1 {
                    let elem = item.elem;
                    writeln!(f, "{namespace}::load_matrix_sync({frag}, reinterpret_cast<{elem} *>({value}), {stride});")
                } else {
                    writeln!(
                        f,
                        "{namespace}::load_matrix_sync({frag}, {value}, {stride});"
                    )
                }
            }
            WmmaInstruction::Load {
                frag,
                value,
                stride,
                layout: Some(layout),
            } => {
                let layout = match layout {
                    FragmentLayout::ColMajor => format!("{namespace}::mem_col_major"),
                    FragmentLayout::RowMajor => format!("{namespace}::mem_row_major"),
                    FragmentLayout::_Dialect(_) => "".to_string(),
                };
                let item = value.item();
                if item.vectorization > 1 {
                    let elem = item.elem;
                    writeln!(f, "{namespace}::load_matrix_sync({frag}, reinterpret_cast<{elem} *>({value}), {stride}, {layout});")
                } else {
                    writeln!(
                        f,
                        "{namespace}::load_matrix_sync({frag}, {value}, {stride}, {layout});"
                    )
                }
            }
            WmmaInstruction::Execute {
                frag_a,
                frag_b,
                frag_c,
                frag_d,
            } => writeln!(
                f,
                "{namespace}::mma_sync({frag_d}, {frag_a}, {frag_b}, {frag_c});"
            ),
            WmmaInstruction::Store {
                output,
                frag,
                stride,
                layout,
            } => {
                let layout = match layout {
                    FragmentLayout::ColMajor => format!("{namespace}::mem_col_major"),
                    FragmentLayout::RowMajor => format!("{namespace}::mem_row_major"),
                    FragmentLayout::_Dialect(_) => "".to_string(),
                };

                let item = output.item();
                if item.vectorization > 1 {
                    let elem = item.elem;
                    writeln!(
                        f,
                        "{namespace}::store_matrix_sync(reinterpret_cast<{elem} *>({output}), {frag}, {stride}, {layout});"
                    )
                } else {
                    writeln!(
                        f,
                        "{namespace}::store_matrix_sync({output}, {frag}, {stride}, {layout});"
                    )
                }
            }
        }
    }
}
