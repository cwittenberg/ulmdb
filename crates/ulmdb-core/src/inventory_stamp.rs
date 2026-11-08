//! Shared scalar invalidation/identity checks. This does not prove certificate issuance.
#[cfg(not(verus_keep_ghost))]
use verus_builtin_macros::verus;
#[cfg(verus_keep_ghost)]
use vstd::prelude::*;

verus! {
/// Zero never authorizes a cached inventory certificate. Fields stay private.
#[derive(Clone,Copy,Debug,Default)]
pub struct Stamp { identity:u64 }
impl Stamp {
    pub closed spec fn view(&self)->u64 {self.identity}

    #[inline]
    pub fn new(identity:u64)->(stamp:Option<Self>)
        ensures match stamp {
            Some(stamp)=>identity!=0 && stamp.view()==identity,
            None=>identity==0,
        },
    {
        if identity==0 {None}else{Some(Self{identity})}
    }
    #[inline]
    pub fn value(&self)->(identity:u64)
        ensures identity==self.view(),
    {self.identity}

    /// Inventory's mutable-access wrapper calls this BEFORE yielding mutation.
    #[inline]
    pub fn invalidate(&mut self)
        ensures final(self).view()==0,
    {self.identity=0;}
}
pub open spec fn eligible(
    inventory:u64,index:u64,index_generation:u128,generation:u128,
    certified_table:u64,table:u64,
)->bool {
    inventory!=0 && index!=0 && inventory==index
    && index_generation!=0 && generation!=0 && index_generation==generation
    && certified_table!=0 && table!=0 && certified_table==table
}
#[inline]
pub fn append_eligible(
    inventory:u64,index:u64,index_generation:u128,generation:u128,
    certified_table:u64,table:u64,
)->(accepted:bool)
    ensures accepted==eligible(inventory,index,index_generation,generation,certified_table,table),
{
    inventory!=0 && index!=0 && inventory==index
    && index_generation!=0 && generation!=0 && index_generation==generation
    && certified_table!=0 && table!=0 && certified_table==table
}
pub proof fn invalidated_stamp_is_ineligible(index:u64,index_generation:u128,generation:u128,certified_table:u64,table:u64)
    ensures !eligible(0,index,index_generation,generation,certified_table,table),
{}
pub proof fn changed_binding_is_ineligible(inventory:u64,index:u64,index_generation:u128,generation:u128,certified_table:u64,table:u64)
    requires inventory!=index || index_generation!=generation || certified_table!=table,
    ensures !eligible(inventory,index,index_generation,generation,certified_table,table),
{}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mutation_invalidates_and_default_never_certifies() {
        let mut stamp = Stamp::new(7).unwrap();
        assert!(append_eligible(stamp.value(), 7, 11, 11, 13, 13));
        stamp.invalidate();
        assert_eq!(stamp.value(), 0);
        assert!(!append_eligible(stamp.value(), 7, 11, 11, 13, 13));
        assert_eq!(Stamp::default().value(), 0);
        assert!(Stamp::new(0).is_none());
        for stamp in 0..=2 {
            for index in 0..=2 {
                for generation in 0..=2 {
                    for old_generation in 0..=2 {
                        for table in 0..=2 {
                            for certified in 0..=2 {
                                assert_eq!(
                                    append_eligible(
                                        stamp,
                                        index,
                                        old_generation,
                                        generation,
                                        certified,
                                        table
                                    ),
                                    stamp != 0
                                        && stamp == index
                                        && generation != 0
                                        && generation == old_generation
                                        && table != 0
                                        && table == certified
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
