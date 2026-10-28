from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from scripts.codex import docs_inventory


def write_doc(root: Path, path: str, metadata: str, body: str = "Body.\n") -> Path:
    target = root / path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(f"# Title\n\n{metadata}\n\n{body}", encoding="utf-8")
    return target


class DocsInventoryTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.repo = Path(self.temporary.name)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def canonical(self, path: str = "docs/A.md", domain: str = "a") -> docs_inventory.Document:
        target = write_doc(
            self.repo,
            path,
            "> 文档角色：canonical\n"
            "> 生命周期状态：current\n"
            f"> 领域键：{domain}\n"
            "> 当前真源：self\n"
            "> 摘要：canonical fixture",
        )
        return docs_inventory.parse_document(self.repo, target)

    def decision(
        self,
        path: str,
        *,
        status: str = "accepted",
        summary: str = "decision fixture",
    ) -> docs_inventory.Document:
        target = write_doc(
            self.repo,
            path,
            "> 文档角色：decision\n"
            f"> 生命周期状态：{status}\n"
            "> 当前真源：docs/A.md\n"
            f"> 摘要：{summary}",
        )
        return docs_inventory.parse_document(self.repo, target)

    def test_parses_and_validates_canonical(self) -> None:
        document = self.canonical()
        self.assertEqual(document.role, "canonical")
        self.assertEqual(document.domains, ["a"])
        self.assertEqual(docs_inventory.validate_inventory(self.repo, [document]), [])

    def test_rejects_duplicate_domain_and_illegal_todo_status(self) -> None:
        first = self.canonical("docs/A.md", "same")
        second = self.canonical("docs/B.md", "same")
        todo = write_doc(
            self.repo,
            "docs/TODO/X.md",
            "> 文档角色：active-change\n"
            "> 生命周期状态：paused\n"
            "> 当前真源：docs/A.md\n"
            "> 摘要：invalid fixture",
        )
        errors = docs_inventory.validate_inventory(
            self.repo, [first, second, docs_inventory.parse_document(self.repo, todo)]
        )
        self.assertTrue(any("exactly one canonical owner" in error for error in errors))
        self.assertTrue(any("invalid lifecycle status" in error for error in errors))

    def test_removed_review_metadata_is_rejected(self) -> None:
        target = write_doc(
            self.repo,
            "docs/A.md",
            "> 文档角色：canonical\n"
            "> 生命周期状态：current\n"
            "> 领域键：a\n"
            "> 当前真源：self\n"
            "> 复核触发：src/**\n"
            "> 摘要：legacy fixture",
        )
        with self.assertRaises(docs_inventory.InventoryError):
            docs_inventory.parse_document(self.repo, target)

    def test_only_canonical_documents_may_own_themselves(self) -> None:
        target = write_doc(
            self.repo,
            "docs/ADR/0001-test.md",
            "> 文档角色：decision\n"
            "> 生命周期状态：accepted\n"
            "> 当前真源：self\n"
            "> 摘要：invalid self-owned decision",
        )
        errors = docs_inventory.validate_document(
            docs_inventory.parse_document(self.repo, target)
        )
        self.assertTrue(any("only canonical documents" in error for error in errors))

    def test_generated_reference_requires_concrete_generator(self) -> None:
        canonical = self.canonical()
        target = write_doc(
            self.repo,
            "docs/GENERATED.md",
            "> 文档角色：generated-reference\n"
            "> 生命周期状态：generated\n"
            "> 当前真源：docs/A.md\n"
            "> 生成器：scripts/*.py\n"
            "> 摘要：generated fixture",
        )
        generated = docs_inventory.parse_document(self.repo, target)
        errors = docs_inventory.validate_inventory(self.repo, [canonical, generated])
        self.assertTrue(any("one concrete repository path" in error for error in errors))

    def test_generated_reference_requires_existing_generator(self) -> None:
        canonical = self.canonical()
        target = write_doc(
            self.repo,
            "docs/GENERATED.md",
            "> 文档角色：generated-reference\n"
            "> 生命周期状态：generated\n"
            "> 当前真源：docs/A.md\n"
            "> 生成器：scripts/missing.py\n"
            "> 摘要：generated fixture",
        )
        generated = docs_inventory.parse_document(self.repo, target)
        errors = docs_inventory.validate_inventory(self.repo, [canonical, generated])
        self.assertTrue(any("generator path does not exist" in error for error in errors))

    def test_generated_tables_are_deterministic(self) -> None:
        first = self.canonical("docs/B.md", "z")
        second = self.canonical("docs/A.md", "a")
        table = docs_inventory.render_canonical_table([first, second])
        self.assertLess(table.index("`a`"), table.index("`z`"))

    def test_generated_decision_index_is_deterministic(self) -> None:
        canonical = self.canonical()
        second = self.decision("docs/ADR/0002-second.md", summary="second decision")
        first = self.decision(
            "docs/ADR/0001-first.md", status="proposed", summary="first decision"
        )
        targets = docs_inventory._generated_targets(self.repo, [canonical, second, first])
        table = targets["decisions"][1]
        self.assertLess(table.index("0001-first.md"), table.index("0002-second.md"))
        self.assertIn("`proposed`", table)
        self.assertIn("first decision", table)

    def test_owner_must_be_governed_current_document(self) -> None:
        canonical = self.canonical()
        reference = write_doc(
            self.repo,
            "docs/R.md",
            "> 文档角色：current-reference\n"
            "> 生命周期状态：current\n"
            "> 当前真源：plain.txt\n"
            "> 摘要：reference with an invalid non-document owner",
        )
        errors = docs_inventory.validate_inventory(
            self.repo, [canonical, docs_inventory.parse_document(self.repo, reference)]
        )
        self.assertTrue(any("governed current document" in error for error in errors))

    def test_role_path_matrix_is_bidirectional(self) -> None:
        target = write_doc(
            self.repo,
            "docs/WRONG.md",
            "> 文档角色：decision\n"
            "> 生命周期状态：accepted\n"
            "> 当前真源：docs/A.md\n"
            "> 摘要：decision deliberately placed outside the ADR directory",
        )
        errors = docs_inventory.validate_document(
            docs_inventory.parse_document(self.repo, target)
        )
        self.assertTrue(any("only allowed under docs/ADR" in error for error in errors))

    def test_archive_requires_local_reason_or_replacement(self) -> None:
        target = write_doc(
            self.repo,
            "docs/ARCHIVE/X.md",
            "> 文档角色：archive\n"
            "> 生命周期状态：historical\n"
            "> 摘要：archive fixture",
        )
        errors = docs_inventory.validate_document(
            docs_inventory.parse_document(self.repo, target)
        )
        self.assertTrue(any("replacement or historical reason" in error for error in errors))

    def test_archive_cannot_own_itself(self) -> None:
        target = write_doc(
            self.repo,
            "docs/ARCHIVE/X.md",
            "> 文档角色：archive\n"
            "> 生命周期状态：historical\n"
            "> 当前真源：self\n"
            "> 历史原因：historical fixture\n"
            "> 摘要：archive fixture",
        )
        errors = docs_inventory.validate_document(
            docs_inventory.parse_document(self.repo, target)
        )
        self.assertTrue(any("only canonical documents" in error for error in errors))

    def test_invalid_blockquote_inside_metadata_fails(self) -> None:
        target = self.repo / "docs/BAD.md"
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(
            "# Bad\n\n> 文档角色：canonical\n> not key value\n> 生命周期状态：current\n",
            encoding="utf-8",
        )
        with self.assertRaises(docs_inventory.InventoryError):
            docs_inventory.parse_document(self.repo, target)


if __name__ == "__main__":
    unittest.main()
