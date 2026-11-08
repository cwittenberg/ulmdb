"""Pure negative controls; these do not set limits or launch a child."""
import copy
import unittest
from controller import classify, validate_ready


class ControllerControls(unittest.TestCase):
    def setUp(self):
        self.ready = {"type":"ready","schema":1,"keys":2_000_000,"max_entries":2_000_000,
                      "value_bytes":8,"max_value_bytes":8,"max_total_value_bytes":16_000_000,
                      "required_index_bytes":160<<20,"index_budget_bytes":512<<20,
                      "full_source_verified":True,"original_native_bound":True,"sequence":1,"commit":"123"}
        self.attempt = {"type":"attempt","outcome":{"kind":"limit_exceeded"},
                        "full_source_verified":True,"native_and_ordered_usable":True,
                        "snapshot_unchanged":True,"original_native_still_usable":True,"sequence":1,"commit":"123"}
        self.retry = copy.deepcopy(self.attempt)
        self.retry.update(type="retry",outcome={"kind":"allocated","index_bytes":160<<20,
                          "entries":2_000_000,"all_hash_values_verified":True})

    def verdict(self):
        return classify(self.ready,self.attempt,self.retry,True,0)[0]

    def test_complete_control_is_accepted(self):
        validate_ready(self.ready,2_000_000,512<<20)
        self.assertEqual(self.verdict(),"pass")

    def test_budget_rejection_cannot_masquerade_as_allocator_refusal(self):
        self.ready["index_budget_bytes"]=1
        with self.assertRaises(ValueError):
            validate_ready(self.ready,2_000_000,1)

    def test_wrong_entry_budget_is_rejected(self):
        self.ready["max_entries"]-=1
        with self.assertRaises(ValueError):
            validate_ready(self.ready,2_000_000,512<<20)

    def test_allocation_success_under_cap_is_inconclusive(self):
        self.attempt["outcome"]={"kind":"allocated"}
        self.assertEqual(self.verdict(),"inconclusive")

    def test_unexpected_error_is_not_a_pass(self):
        self.attempt["outcome"]={"kind":"unexpected_error","error":"unrelated"}
        self.assertEqual(self.verdict(),"fail")

    def test_recovery_is_required(self):
        self.retry["outcome"]={"kind":"unexpected_error"}
        self.assertEqual(self.verdict(),"fail")

    def test_original_snapshot_identity_is_required(self):
        self.attempt["commit"]="different"
        self.assertEqual(self.verdict(),"fail")

    def test_original_source_usability_is_required(self):
        self.attempt["native_and_ordered_usable"]=False
        self.assertEqual(self.verdict(),"fail")

    def test_existing_heap_reuse_is_not_a_proof(self):
        self.assertEqual(classify(self.ready,self.attempt,self.retry,False,0)[0],"inconclusive")

    def test_panic_or_signal_is_not_a_pass(self):
        self.assertEqual(classify(self.ready,self.attempt,self.retry,True,-6)[0],"fail")


if __name__ == "__main__":
    unittest.main()
