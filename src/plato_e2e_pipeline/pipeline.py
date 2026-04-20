"""End-to-end tile processing pipeline."""

import time, uuid
from collections import Counter

class E2EPipeline:
    def __init__(self):
        self._store: dict[str, dict] = {}
        self._processed = 0
        self._dedup_count = 0

    def process(self, tile: dict) -> dict:
        self._processed += 1
        errors = []
        content = tile.get("content", "")
        confidence = tile.get("confidence", 0.5)

        if len(content) < 10: errors.append("content too short")
        if not (0 <= confidence <= 1): errors.append("confidence out of range")

        score = 0.0
        if not errors:
            score = confidence * 0.5 + min(len(content) / 500, 1.0) * 0.3 + 0.2

        result = {"id": tile.get("id", str(uuid.uuid4())[:8]), "content": content,
                  "domain": tile.get("domain", "general"), "confidence": confidence,
                  "score": score, "errors": errors, "valid": len(errors) == 0}

        if result["valid"]:
            self._store[result["id"]] = result

        return result

    def process_batch(self, tiles: list[dict]) -> list[dict]:
        return [self.process(t) for t in tiles]

    def search(self, query: str, top_n: int = 5) -> list[dict]:
        q_words = set(query.lower().split())
        results = []
        for t in self._store.values():
            c_words = set(t["content"].lower().split())
            if q_words and c_words:
                overlap = len(q_words & c_words) / len(q_words | c_words)
                t["_search_score"] = overlap
                results.append(t)
        results.sort(key=lambda x: -x.get("_search_score", 0))
        return results[:top_n]

    def dedup_all(self, threshold: float = 0.9) -> int:
        to_remove = []
        ids = list(self._store.keys())
        for i in range(len(ids)):
            for j in range(i + 1, len(ids)):
                a = self._store[ids[i]]["content"].lower().split()
                b = self._store[ids[j]]["content"].lower().split()
                if a and b:
                    jaccard = len(set(a) & set(b)) / len(set(a) | set(b))
                    if jaccard >= threshold:
                        to_remove.append(ids[j])
        for rid in set(to_remove):
            del self._store[rid]
            self._dedup_count += 1
        return len(to_remove)

    @property
    def stats(self) -> dict:
        domains = dict(Counter(t.get("domain", "unknown") for t in self._store.values()))
        scores = [t["score"] for t in self._store.values()]
        return {"total_stored": len(self._store), "total_processed": self._processed,
                "dedup_count": self._dedup_count,
                "avg_score": sum(scores) / len(scores) if scores else 0,
                "domains": domains}
