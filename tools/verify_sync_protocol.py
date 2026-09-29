"""云同步研究的因果版本验证模型；不读数据库、不访问网络，不是生产同步实现。

运行：python tools/verify_sync_protocol.py
仅验证单实体批次；多表事务、凭据、加密、提供者与 UI 需另外用生产代码验收。
"""
import itertools
import json
import unittest
from dataclasses import dataclass, replace


@dataclass(frozen=True)
class Event:
    workspace: str
    device: str
    sequence: int
    seen: tuple
    entity: str
    operation: str
    value_json: str

    @property
    def clock(self):
        return dict(self.seen) | {self.device: self.sequence}

    @property
    def identity(self):
        return self.device, self.sequence


def dominates(left, right):
    return all(left.get(device, 0) >= sequence for device, sequence in right.items())


class Replica:
    def __init__(self, device, workspace="synthetic-workspace"):
        self.device = device
        self.workspace = workspace
        self.observed = {}
        self.events = {}
        self.heads = {}

    def receive(self, event):
        # 研究模型只接收 Event；实际不可信 JSON / AEAD 校验不在这里。
        if event.workspace != self.workspace:
            raise ValueError("different workspace")
        seen = dict(event.seen)
        if (not event.device or not event.entity or len(seen) != len(event.seen)
                or type(event.sequence) is not int or not 1 <= event.sequence <= 2**53 - 1
                or any(not device or type(seq) is not int or not 0 <= seq <= 2**53 - 1
                       for device, seq in event.seen)
                or seen.get(event.device, 0) != event.sequence - 1
                or event.operation not in ("upsert", "delete")):
            raise ValueError("invalid event")
        value = json.loads(event.value_json)
        if event.operation == "delete" and value is not None:
            raise ValueError("delete cannot have payload")
        existing = self.events.get(event.identity)
        if existing is not None:
            if existing != event:
                raise ValueError("same identity with different content")
            return False
        if event.sequence != self.observed.get(event.device, 0) + 1:
            raise ValueError("sequence gap")
        if not dominates(self.observed, seen):
            raise ValueError("missing causal history")
        retained = [old for old in self.heads.get(event.entity, [])
                    if not dominates(event.clock, old.clock)]
        self.heads[event.entity] = retained + [event]
        self.events[event.identity] = event
        self.observed[event.device] = event.sequence
        return True

    def commit(self, entity, value=None, operation="upsert"):
        event = Event(self.workspace, self.device,
                      self.observed.get(self.device, 0) + 1,
                      tuple(sorted(self.observed.items())), entity, operation,
                      json.dumps(value, sort_keys=True, ensure_ascii=False))
        self.receive(event)
        return event

    def variants(self, entity):
        # 相同内容只显示一份，底层保留所有因果来源。
        return {(event.operation, event.value_json) for event in self.heads.get(entity, [])}


def two_offline_edits():
    home, office = Replica("home"), Replica("office")
    baseline = home.commit("memo:own-example", {"title": "合成备忘", "body": "初稿"})
    office.receive(baseline)
    left = home.commit("memo:own-example", {"title": "合成备忘", "body": "家里修改"})
    right = office.commit("memo:own-example", {"title": "合成备忘", "body": "公司修改"})
    return home, office, left, right


class ProtocolChecks(unittest.TestCase):
    def test_causal_successor_replaces_observed_version(self):
        home, office = Replica("home"), Replica("office")
        office.receive(home.commit("memo:1", "first"))
        home.receive(office.commit("memo:1", "later"))
        self.assertEqual(home.variants("memo:1"), {("upsert", '"later"')})

    def test_offline_edits_preserve_both_versions(self):
        home, office, left, right = two_offline_edits()
        home.receive(right)
        office.receive(left)
        self.assertEqual(home.variants(left.entity), office.variants(left.entity))
        self.assertEqual(len(home.variants(left.entity)), 2)

    def test_three_concurrent_devices_preserve_three_versions(self):
        events = [Replica(device).commit("memo:1", device) for device in ("A", "B", "C")]
        observer = Replica("reader")
        for event in events:
            observer.receive(event)
        self.assertEqual(len(observer.variants("memo:1")), 3)

    def test_independent_records_are_both_retained(self):
        home, office = Replica("home"), Replica("office")
        one, two = home.commit("memo:1", "one"), office.commit("memo:2", "two")
        home.receive(two)
        office.receive(one)
        self.assertEqual(home.heads, office.heads)

    def test_identical_concurrent_content_keeps_causal_origins(self):
        one, two = Replica("A"), Replica("B")
        event_a, event_b = one.commit("memo:1", "same"), two.commit("memo:1", "same")
        one.receive(event_b)
        two.receive(event_a)
        self.assertEqual(len(one.variants("memo:1")), 1)
        self.assertEqual(len(one.heads["memo:1"]), 2)

    def test_replay_is_idempotent(self):
        one, two = Replica("A"), Replica("B")
        event = one.commit("memo:1", "example")
        self.assertTrue(two.receive(event))
        self.assertFalse(two.receive(event))
        self.assertEqual(len(two.events), 1)

    def test_reused_identity_different_bytes_is_rejected_without_change(self):
        replica = Replica("A")
        event = replica.commit("memo:1", "safe")
        with self.assertRaisesRegex(ValueError, "different content"):
            replica.receive(replace(event, value_json='"bad"'))
        self.assertEqual(replica.events[event.identity], event)
        self.assertEqual(len(replica.events), 1)

    def test_sequence_gap_waits_for_predecessor(self):
        one, two = Replica("A"), Replica("B")
        first = one.commit("memo:1", "first")
        second = one.commit("memo:1", "second")
        with self.assertRaisesRegex(ValueError, "sequence gap"):
            two.receive(second)
        self.assertFalse(two.events)
        two.receive(first)
        two.receive(second)
        self.assertEqual(two.variants("memo:1"), {("upsert", '"second"')})

    def test_cross_device_causal_gap_is_not_skipped(self):
        one, two, third = Replica("A"), Replica("B"), Replica("C")
        baseline = one.commit("memo:1", "first")
        two.receive(baseline)
        successor = two.commit("memo:1", "second")
        with self.assertRaisesRegex(ValueError, "missing causal"):
            third.receive(successor)
        self.assertFalse(third.events)
        third.receive(baseline)
        third.receive(successor)
        self.assertEqual(len(third.heads["memo:1"]), 1)

    def test_foreign_workspace_is_rejected(self):
        replica = Replica("A")
        with self.assertRaisesRegex(ValueError, "workspace"):
            replica.receive(Replica("B", "other").commit("memo:1", "foreign"))
        self.assertFalse(replica.events)

    def test_delete_and_concurrent_edit_remain_a_conflict(self):
        home, office = Replica("home"), Replica("office")
        office.receive(home.commit("memo:1", "first"))
        deleted = home.commit("memo:1", operation="delete")
        edited = office.commit("memo:1", "concurrent edit")
        home.receive(edited)
        office.receive(deleted)
        self.assertEqual(home.variants("memo:1"), office.variants("memo:1"))
        self.assertEqual({kind for kind, _ in home.variants("memo:1")}, {"upsert", "delete"})

    def test_observed_delete_supersedes_old_content(self):
        replica = Replica("A")
        replica.commit("memo:1", "first")
        replica.commit("memo:1", operation="delete")
        self.assertEqual(replica.variants("memo:1"), {("delete", "null")})

    def test_resolution_observes_all_variants_and_converges(self):
        home, office, left, right = two_offline_edits()
        home.receive(right)
        office.receive(left)
        resolved = home.commit(left.entity, {"title": "合成备忘", "body": "合并后的内容"})
        office.receive(resolved)
        self.assertEqual(home.variants(left.entity), office.variants(left.entity))
        self.assertEqual(len(office.heads[left.entity]), 1)

    def test_delivery_permutations_converge(self):
        events = [Replica(device).commit("memo:1", {"value": device})
                  for device in ("A", "B", "C")]
        expected = {(event.operation, event.value_json) for event in events}
        for order in itertools.permutations(events):
            replica = Replica("observer")
            for event in order:
                replica.receive(event)
            self.assertEqual(replica.variants("memo:1"), expected)

    def test_invalid_own_predecessor_is_rejected(self):
        replica = Replica("A")
        valid = Replica("B").commit("memo:1", "example")
        with self.assertRaisesRegex(ValueError, "invalid event"):
            replica.receive(replace(valid, seen=(("B", 5),)))
        self.assertFalse(replica.events)

    def test_delete_payload_is_rejected(self):
        replica = Replica("A")
        valid = Replica("B").commit("memo:1", "example")
        with self.assertRaisesRegex(ValueError, "delete"):
            replica.receive(replace(valid, operation="delete"))
        self.assertFalse(replica.events)


if __name__ == "__main__":
    unittest.main(verbosity=2)
