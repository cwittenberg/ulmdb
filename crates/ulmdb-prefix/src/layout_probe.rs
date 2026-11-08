use crate::{node::Node, reader::Reader};
use ulmdb_core::{
    Address, Prefix,
    storage::{Error, ReadStore, Reference, Result},
};
struct Empty;
impl ReadStore for Empty {
    fn read(&self, _: Reference, _: usize) -> Result<&[u8]> {
        Err(Error::Corrupt("layout-only"))
    }
}
#[allow(dead_code)]
struct Scalar4 {
    network: u32,
    length: u8,
    children: [Reference; 2],
    value: Reference,
}
#[allow(dead_code)]
struct Scalar6 {
    network: u128,
    length: u8,
    children: [Reference; 2],
    value: Reference,
}
#[allow(dead_code)]
enum ScalarRoot {
    Empty,
    V4(Scalar4),
    V6(Scalar6),
}
#[test]
fn layout_only_actual_and_proposed_scalar_types() {
    macro_rules! show {
        ($t:ty) => {
            println!(
                "LAYOUT {} size={} align={}",
                stringify!($t),
                std::mem::size_of::<$t>(),
                std::mem::align_of::<$t>()
            )
        };
    }
    show!(Address);
    show!(Prefix);
    show!(Reference);
    show!(Node);
    show!(Option<Node>);
    show!(Result<Node>);
    show!(Reader<'_, Empty>);
    show!(Result<Option<(Prefix, &[u8])>>);
    show!(Scalar4);
    show!(Scalar6);
    show!(ScalarRoot);
    #[cfg(feature = "scalar-family")]
    {
        show!(crate::scalar_lookup::ScalarNode<u32>);
        show!(crate::scalar_lookup::ScalarNode<u128>);
        show!(Result<crate::scalar_lookup::ScalarNode<u32>>);
        show!(Result<crate::scalar_lookup::ScalarNode<u128>>);
        show!(crate::scalar_lookup::Root);
        show!(crate::scalar_reader::Reader<'_, Empty>);
    }
}
