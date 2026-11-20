"""Positive and negative controls for the mandatory PR maintenance contract."""
import json
from pathlib import Path
import tempfile
import unittest

from check_design import check_record, check_document, check_svg


class DesignControls(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.visual = 'docs/design/flow.md'
        self.path = self.root / self.visual
        self.path.parent.mkdir(parents=True)
        self.path.write_text('# Flow\n\n```plantuml\n@startuml\nA -> B : validated call\n@enduml\n```\n')
        self.changed = {'src/module.py', self.visual}
        self.record = {'mappings': [{'sources': ['src/module.py'], 'visual': self.visual,
            'reason': 'Compared final call owner with flow; topology is unchanged.'}],
            'validation': 'documd 6.0.0 rendered all views; inspected readable labels and arrows.'}

    def body(self):
        return '<!-- diagram-maintenance\n' + json.dumps(self.record) + '\ndiagram-maintenance -->'

    def test_behavior_change_passes(self):
        check_record(self.root, self.body(), self.changed)
        self.assertEqual(check_document(self.root, self.path), 1)

    def test_unchanged_topology_provenance_passes(self):
        self.path.write_text(self.path.read_text() + '\nSource review: module error handling preserves caller/owner boundary.\n')
        check_record(self.root, self.body(), self.changed)

    def test_missing_record_fails(self):
        with self.assertRaisesRegex(ValueError, 'record is required'):
            check_record(self.root, 'No diagrams applicable', self.changed)

    def test_no_visual_update_fails(self):
        with self.assertRaisesRegex(ValueError, 'must exist and change'):
            check_record(self.root, self.body(), {'src/module.py'})

    def test_unmapped_source_fails(self):
        with self.assertRaisesRegex(ValueError, 'unmapped'):
            check_record(self.root, self.body(), self.changed | {'src/other.py'})

    def test_no_mapping_fails(self):
        self.record['mappings'] = []
        with self.assertRaisesRegex(ValueError, 'nonempty mappings'):
            check_record(self.root, self.body(), self.changed)

    def test_unsupported_diagram_fails(self):
        self.path.write_text('```mermaid\ngraph TD\n```\n')
        with self.assertRaisesRegex(ValueError, 'unsupported fence'):
            check_document(self.root, self.path)

    def test_tilde_diagram_fence_fails(self):
        self.path.write_text('~~~plantuml\n@startuml\nA -> B\n@enduml\n~~~\n')
        with self.assertRaisesRegex(ValueError, 'canonical unindented'):
            check_document(self.root, self.path)

    def test_indented_diagram_fence_fails(self):
        self.path.write_text('  ```plantuml\n@startuml\nA -> B\n@enduml\n  ```\n')
        with self.assertRaisesRegex(ValueError, 'canonical unindented'):
            check_document(self.root, self.path)

    def test_incomplete_diagram_fails(self):
        self.path.write_text('```plantuml\n@startuml\nA -> B\n```\n')
        with self.assertRaisesRegex(ValueError, 'complete UML'):
            check_document(self.root, self.path)

    def test_malformed_braces_fail(self):
        self.path.write_text('```plantuml\n@startuml\nclass Broken {\n@enduml\n```\n')
        with self.assertRaisesRegex(ValueError, 'unbalanced'):
            check_document(self.root, self.path)

    def test_labelled_foreign_object_render_passes(self):
        svg = self.root / 'labelled.svg'
        svg.write_text('<svg xmlns="http://www.w3.org/2000/svg"><foreignObject><div xmlns="http://www.w3.org/1999/xhtml">Caller</div></foreignObject></svg>')
        check_svg(svg)

    def test_empty_render_fails(self):
        svg = self.root / 'empty.svg'
        svg.write_text('<svg xmlns="http://www.w3.org/2000/svg"><g/></svg>')
        with self.assertRaisesRegex(ValueError, 'empty/unlabelled'):
            check_svg(svg)

    def test_missing_link_fails(self):
        self.path.write_text('[Missing](../../absent.py)')
        with self.assertRaisesRegex(ValueError, 'broken/escaping'):
            check_document(self.root, self.path)

    def test_external_include_fails(self):
        self.path.write_text('```plantuml\n@startuml\n!include remote\n@enduml\n```\n')
        with self.assertRaisesRegex(ValueError, 'inline'):
            check_document(self.root, self.path)

    def test_diagram_only_pr_passes(self):
        self.record['mappings'][0]['sources'] = [self.visual]
        check_record(self.root, self.body(), {self.visual})

    def test_deleted_source_requires_mapping(self):
        # Deleted paths need not exist, but must be accounted for in the PR.
        self.record['mappings'][0]['sources'] = ['deleted.py']
        check_record(self.root, self.body(), {'deleted.py', self.visual})


if __name__ == '__main__':
    unittest.main(verbosity=2)
