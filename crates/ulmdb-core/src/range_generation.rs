//! Scoped nonzero physical IDs, injective across positive ordinals per file.
//! Logical commit IDs and hash collision resistance are separate contracts.
#[cfg(not(verus_keep_ghost))]
use verus_builtin_macros::verus;
#[cfg(verus_keep_ghost)]
use vstd::prelude::*;
verus! {
pub open spec fn physical_id(mask:u128,ordinal:u64)->u128 {
 let mixed=mask^(ordinal as u128);if mixed==0{mask}else{mixed}
}
#[inline]
pub fn physical_generation(mask:u128,ordinal:u64)->(id:Option<u128>)
 ensures match id {
  Some(id)=>mask!=0 && ordinal!=0 && id!=0 && id==physical_id(mask,ordinal),
  None=>mask==0 || ordinal==0,
 },
{
 if mask==0 || ordinal==0 {return None;}
 let mixed=mask^(ordinal as u128);
 if mixed==0{Some(mask)}else{Some(mixed)}
}
pub proof fn positive_ordinals_are_injective(mask:u128,a:u64,b:u64)
 requires mask!=0,a!=0,b!=0,physical_id(mask,a)==physical_id(mask,b),
 ensures a==b,
{
 let x=mask^(a as u128);let y=mask^(b as u128);
 if x==0 {
  if y!=0 {assert(y!=mask) by(bit_vector) requires b!=0,y==mask^(b as u128);}
  assert(a==b) by(bit_vector) requires x==0,y==0,x==mask^(a as u128),y==mask^(b as u128);
 }else if y==0 {
  assert(x!=mask) by(bit_vector) requires a!=0,x==mask^(a as u128);
 }else {
  assert(a==b) by(bit_vector) requires x==y,x==mask^(a as u128),y==mask^(b as u128);
 }
}
pub proof fn fixed_ordinal_preserves_all_nonzero_file_id_entropy(a:u128,b:u128,o:u64)
 requires a!=0,b!=0,o!=0,physical_id(a,o)==physical_id(b,o),
 ensures a==b,
{
 let x=a^(o as u128);let y=b^(o as u128);
 if x==0 {
  if y!=0 {assert(y!=a) by(bit_vector) requires b!=0,x==0,x==a^(o as u128),y==b^(o as u128);}
  assert(a==b) by(bit_vector) requires x==0,y==0,x==a^(o as u128),y==b^(o as u128);
 }else if y==0 {
  assert(x!=b) by(bit_vector) requires a!=0,y==0,x==a^(o as u128),y==b^(o as u128);
 }else {
  assert(a==b) by(bit_vector) requires x==y,x==a^(o as u128),y==b^(o as u128);
 }
}
pub proof fn fixed_ordinal_mapping_is_its_own_inverse(mask:u128,o:u64)
 requires mask!=0,o!=0,
 ensures physical_id(physical_id(mask,o),o)==mask,
{
 let x=mask^(o as u128);
 if x==0 {}else{assert((x^(o as u128))==mask) by(bit_vector) requires x==mask^(o as u128);}
}

}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_zero_and_exceptional_xor_zero_are_exact() {
        assert_eq!(physical_generation(0, 1), None);
        assert_eq!(physical_generation(1, 0), None);
        for n in [1, 2, 127, u64::MAX] {
            assert_eq!(physical_generation(n as u128, n), Some(n as u128));
        }
    }
    #[test]
    fn boundary_and_many_scoped_ids_never_repeat() {
        for mask in [1, 2, 1u128 << 64, u64::MAX as u128, u128::MAX] {
            let mut ids = std::collections::BTreeSet::new();
            for ordinal in (1..1024).chain([u64::MAX - 1, u64::MAX]) {
                assert!(ids.insert(physical_generation(mask, ordinal).unwrap()));
                assert_ne!(physical_generation(mask, ordinal), Some(0));
            }
        }
    }
}
