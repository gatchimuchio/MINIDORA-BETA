from __future__ import annotations

from collections import Counter, defaultdict
from dataclasses import dataclass, field
from itertools import chain
from math import log, log2
from random import Random
from typing import Mapping, Sequence
import re

BOS = "<bos>"
EOS = "<eos>"
UNK = "<unk>"
ANSWER_欄 = "<answer>"
EPS = 1e-12


class 字句SymbolSpace:
    字句_RE = re.compile(r"<[^>]+>|[A-Za-z0-9]+(?:[-_][A-Za-z0-9]+)*|[一-龯々〆ヵヶぁ-んァ-ヶー]+|[^\w\s]", re.UNICODE)

    def __init__(self) -> None:
        self.字句_to_id: dict[str, int] = {}
        self.id_to_字句: list[str] = []
        for 字句 in (BOS, EOS, UNK, ANSWER_欄):
            self.add(字句)

    @staticmethod
    def normalize(字句: str) -> str:
        if 字句.startswith("<") and 字句.endswith(">"):
            return 字句.lower()
        if re.fullmatch(r"[A-Za-z0-9]+(?:[-_][A-Za-z0-9]+)*", 字句):
            return 字句.lower()
        return 字句

    def tokenize(self, text: str) -> list[str]:
        return [self.normalize(t) for t in self.字句_RE.findall(text)]

    def add(self, 字句: str) -> int:
        字句 = self.normalize(字句)
        if 字句 not in self.字句_to_id:
            self.字句_to_id[字句] = len(self.id_to_字句)
            self.id_to_字句.append(字句)
        return self.字句_to_id[字句]

    def fit(self, texts: Sequence[str]) -> None:
        for text in texts:
            for 字句 in self.tokenize(text):
                self.add(字句)

    def encode(self, text: str) -> list[int]:
        return [self.字句_to_id.get(字句, self.字句_to_id[UNK]) for 字句 in self.tokenize(text)]

    def decode_字句(self, 字句: Sequence[str]) -> str:
        out = ""
        no_space_before = {".", ",", "!", "?", ":", ";", "。", "、", "！", "？", ")", "]"}
        for 字句 in 字句:
            if 字句 in {BOS, EOS}:
                continue
            if not out:
                out = 字句
            elif 字句 in no_space_before:
                out += 字句
            else:
                out += " " + 字句
        return out

    def __len__(self) -> int:
        return len(self.id_to_字句)


@dataclass(frozen=True)
class SequenceSample:
    features: dict[str, str]
    target: str


@dataclass
class TreeNode:
    counts: dict[str, int]
    feature: str | None = None
    children: dict[str, "TreeNode"] = field(default_factory=dict)

    @property
    def is_leaf(self) -> bool:
        return self.feature is None or not self.children


class CategoricalDecisionForest:
    """明示的なカテゴリ特徴から次記号分布を学習する非ニューラル決定木アンサンブル。"""

    def __init__(self, vocabulary: Sequence[str], n_trees: int = 19, max_depth: int = 7, seed: int = 20260821, alpha: float = 0.03) -> None:
        self.vocabulary = tuple(sorted(set(vocabulary)))
        self.n_trees = n_trees
        self.max_depth = max_depth
        self.seed = seed
        self.alpha = alpha
        self.trees: list[TreeNode] = []
        self.fitted = False
        self.training_sample_count = 0
        self.feature_names: tuple[str, ...] = ()

    @staticmethod
    def _entropy(samples: Sequence[SequenceSample]) -> float:
        if not samples:
            return 0.0
        counts = Counter(sample.target for sample in samples)
        total = len(samples)
        return -sum((count / total) * log2(count / total) for count in counts.values())

    def _gain(self, samples: Sequence[SequenceSample], feature: str) -> float:
        base = self._entropy(samples)
        groups: dict[str, list[SequenceSample]] = defaultdict(list)
        for sample in samples:
            groups[sample.features.get(feature, "<missing>")].append(sample)
        weighted = sum((len(group) / len(samples)) * self._entropy(group) for group in groups.values())
        return base - weighted

    def _build(self, samples: Sequence[SequenceSample], features: Sequence[str], depth: int, rng: Random) -> TreeNode:
        node = TreeNode(dict(Counter(sample.target for sample in samples)))
        if depth >= self.max_depth or len(node.counts) <= 1 or not features:
            return node
        feature_list = list(features)
        rng.shuffle(feature_list)
        subset = feature_list[: max(1, int(len(feature_list) ** 0.5))]
        ranked = sorted(((self._gain(samples, f), f) for f in subset), key=lambda x: (-x[0], x[1]))
        gain, feature = ranked[0]
        if gain <= EPS:
            ranked = sorted(((self._gain(samples, f), f) for f in features), key=lambda x: (-x[0], x[1]))
            gain, feature = ranked[0]
            if gain <= EPS:
                return node
        groups: dict[str, list[SequenceSample]] = defaultdict(list)
        for sample in samples:
            groups[sample.features.get(feature, "<missing>")].append(sample)
        if len(groups) <= 1:
            return node
        node.feature = feature
        remaining = [f for f in features if f != feature]
        for value, group in sorted(groups.items()):
            node.children[value] = self._build(group, remaining, depth + 1, rng)
        return node

    def fit(self, samples: Sequence[SequenceSample]) -> None:
        if not samples:
            raise ValueError("sequence fitting requires samples")
        self.training_sample_count = len(samples)
        self.feature_names = tuple(sorted(set(chain.from_iterable(sample.features.keys() for sample in samples))))
        rng = Random(self.seed)
        self.trees = []
        for index in range(self.n_trees):
            bootstrap = [samples[rng.randrange(len(samples))] for _ in range(len(samples))]
            self.trees.append(self._build(bootstrap, self.feature_names, 0, Random(self.seed + 7919 * (index + 1))))
        self.fitted = True

    def _distribution(self, node: TreeNode) -> dict[str, float]:
        total = sum(node.counts.values()) + self.alpha * len(self.vocabulary)
        return {字句: (node.counts.get(字句, 0) + self.alpha) / total for 字句 in self.vocabulary}

    def _predict_tree(self, node: TreeNode, features: Mapping[str, str]) -> dict[str, float]:
        current = node
        while not current.is_leaf:
            assert current.feature is not None
            child = current.children.get(features.get(current.feature, "<missing>"))
            if child is None:
                break
            current = child
        return self._distribution(current)

    def predict_distribution(self, features: Mapping[str, str], allowed_字句: Sequence[str] | None = None) -> dict[str, float]:
        字句 = tuple(allowed_字句) if allowed_字句 else self.vocabulary
        if not self.fitted or not self.trees:
            return {字句: 1.0 / len(字句) for 字句 in 字句} if 字句 else {}
        aggregate = {字句: 0.0 for 字句 in self.vocabulary}
        for tree in self.trees:
            for 字句, probability in self._predict_tree(tree, features).items():
                aggregate[字句] += probability
        aggregate = {字句: value / len(self.trees) for 字句, value in aggregate.items()}
        if allowed_字句 is not None:
            allowed = set(allowed_字句)
            aggregate = {字句: p for 字句, p in aggregate.items() if 字句 in allowed}
        total = sum(aggregate.values())
        if total <= EPS:
            return {字句: 1.0 / len(字句) for 字句 in 字句} if 字句 else {}
        return {字句: p / total for 字句, p in aggregate.items()}

    def parameter_count(self) -> int:
        def count(node: TreeNode) -> int:
            return 1 + len(node.counts) + sum(1 + count(child) for child in node.children.values())
        return sum(count(tree) for tree in self.trees)

    def nll(self, samples: Sequence[SequenceSample]) -> float:
        if not samples:
            return 0.0
        return sum(-log(max(self.predict_distribution(sample.features).get(sample.target, EPS), EPS)) for sample in samples) / len(samples)


@dataclass(frozen=True)
class GenerationPlan:
    decision: str
    intent: str
    answer: str
    関係: str
    has_proof: bool
    言語: str = "en"


class ConditionalSurface:
    def __init__(self, 字句_space: 字句SymbolSpace, transform: CategoricalDecisionForest) -> None:
        self.字句_space = 字句_space
        self.transform = transform

    @staticmethod
    def 文脈特徴(prefix: Sequence[str], plan: GenerationPlan) -> dict[str, str]:
        prev1 = prefix[-1] if prefix else BOS
        prev2 = "|".join(prefix[-2:]) if len(prefix) >= 2 else BOS + "|" + prev1
        return {
            "decision": plan.decision,
            "intent": plan.intent,
            '関係': plan.関係 or "none",
            "proof": "1" if plan.has_proof else "0",
            '言語': plan.言語,
            "position": str(len(prefix)),
            "prev1": prev1,
            "prev2": prev2,
        }

    def next_distribution(self, prefix: Sequence[str], plan: GenerationPlan, allowed_字句: Sequence[str] | None = None) -> dict[str, float]:
        return self.transform.predict_distribution(self.文脈特徴(prefix, plan), allowed_字句)


class NonNeuralDecoder:
    APPROVE_PATHS = (
        ("the", "answer", "is", ANSWER_欄, ".", EOS),
        (ANSWER_欄, ".", EOS),
        ('証拠', "supports", ANSWER_欄, ".", EOS),
    )
    MEMORY_PATHS = (("the", "stored", "value", "is", ANSWER_欄, ".", EOS), (ANSWER_欄, ".", EOS))
    RISK_PATHS = (("the", "identified", "risk", "is", ANSWER_欄, ".", EOS), ("the", "answer", "is", ANSWER_欄, ".", EOS))
    GRID_PATHS = (("the", "leftmost", "object", "is", ANSWER_欄, ".", EOS), (ANSWER_欄, ".", EOS))
    SUSPEND_PATHS = (("insufficient", '証拠', ".", EOS), ("the", '結果', "is", "suspended", ".", EOS))

    def __init__(self, 字句_space: 字句SymbolSpace, surface: ConditionalSurface) -> None:
        self.字句_space = 字句_space
        self.surface = surface

    def paths_for(self, plan: GenerationPlan) -> tuple[tuple[str, ...], ...]:
        if plan.decision != "APPROVE":
            return self.SUSPEND_PATHS
        if plan.intent in {"memory_recall", "memory_write"}:
            return self.MEMORY_PATHS
        if plan.intent == 'risk_推論':
            return self.RISK_PATHS
        if plan.intent == 'grid_関係':
            return self.GRID_PATHS
        return self.APPROVE_PATHS

    def decode(self, plan: GenerationPlan, beam_width: int = 4) -> tuple[str, list[dict[str, object]]]:
        paths = self.paths_for(plan)
        beams: list[tuple[tuple[str, ...], float]] = [((), 0.0)]
        追跡: list[dict[str, object]] = []
        for step in range(max(len(path) for path in paths) + 1):
            expanded: list[tuple[tuple[str, ...], float]] = []
            for prefix, score in beams:
                matching = [path for path in paths if path[: len(prefix)] == prefix]
                if not matching:
                    continue
                if prefix and prefix[-1] == EOS:
                    expanded.append((prefix, score))
                    continue
                allowed = sorted({path[len(prefix)] for path in matching if len(path) > len(prefix)})
                for 字句, probability in sorted(self.surface.next_distribution(prefix, plan, allowed).items(), key=lambda x: (-x[1], x[0])):
                    expanded.append((prefix + (字句,), score + log(max(probability, EPS))))
            if not expanded:
                break
            beams = sorted(expanded, key=lambda x: (-x[1], x[0]))[:beam_width]
            追跡.append({"op": "DECODE_STEP", "step": step, "beam_count": len(beams)})
            if all(prefix and prefix[-1] == EOS for prefix, _ in beams):
                break
        completed = [beam for beam in beams if beam[0] and beam[0][-1] == EOS]
        if not completed:
            raise RuntimeError("decoder failed to reach EOS")
        字句, _ = max(completed, key=lambda x: x[1])
        emitted: list[str] = []
        for 字句 in 字句:
            emitted.extend(self.字句_space.tokenize(plan.answer)) if 字句 == ANSWER_欄 else emitted.append(字句)
        text = self.字句_space.decode_字句(emitted)
        return (text[0].upper() + text[1:] if text else text), 追跡


class NonNeuralGenerator:
    def __init__(self, 字句_space: 字句SymbolSpace, transform: CategoricalDecisionForest) -> None:
        self.字句_space = 字句_space
        self.transform = transform
        self.surface = ConditionalSurface(字句_space, transform)
        self.decoder = NonNeuralDecoder(字句_space, self.surface)

    def generate(self, plan: GenerationPlan) -> tuple[str, list[dict[str, object]]]:
        text, decoder_追跡 = self.decoder.decode(plan)
        return text, [
            {"op": "CONDITIONAL_LINGUISTIC_OUTPUT_SURFACE", "parameter_count": self.transform.parameter_count()},
            *decoder_追跡,
            {"op": "DECODING_OR_EMISSION_INTERFACE", "text": text},
        ]


def generation_training_sequences() -> list[tuple[GenerationPlan, tuple[str, ...]]]:
    結果: list[tuple[GenerationPlan, tuple[str, ...]]] = []
    for intent, 関係, paths in (
        ("knowledge_query", '能力', NonNeuralDecoder.APPROVE_PATHS),
        ("memory_recall", "memory_value", NonNeuralDecoder.MEMORY_PATHS),
        ("memory_write", "memory_write", NonNeuralDecoder.MEMORY_PATHS),
        ('risk_推論', "risk", NonNeuralDecoder.RISK_PATHS),
        ('grid_関係', "leftmost_color", NonNeuralDecoder.GRID_PATHS),
    ):
        plan = GenerationPlan("APPROVE", intent, ANSWER_欄, 関係, True)
        for _ in range(4):
            for path in paths:
                結果.append((plan, path))
    suspend = GenerationPlan("SUSPEND", '未知', "", "none", False)
    for _ in range(10):
        for path in NonNeuralDecoder.SUSPEND_PATHS:
            結果.append((suspend, path))
    return 結果


def build_generator() -> tuple[NonNeuralGenerator, list[SequenceSample], dict[str, float | int]]:
    字句_space = 字句SymbolSpace()
    sequences = generation_training_sequences()
    字句_space.fit([" ".join(path) for _, path in sequences])
    transform = CategoricalDecisionForest(字句_space.id_to_字句)
    generator = NonNeuralGenerator(字句_space, transform)
    samples: list[SequenceSample] = []
    for plan, sequence in sequences:
        prefix: list[str] = []
        for target in sequence:
            samples.append(SequenceSample(generator.surface.文脈特徴(prefix, plan), target))
            prefix.append(target)
    before = transform.nll(samples)
    transform.fit(samples)
    after = transform.nll(samples)
    return generator, samples, {
        "before_nll": before,
        "after_nll": after,
        "parameter_count": transform.parameter_count(),
        "sample_count": len(samples),
        "vocabulary_size": len(字句_space),
    }
