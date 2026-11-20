#!/usr/bin/env python3
"""Repository-local PR design contract. Structural evidence, not semantic proof."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
from urllib.parse import unquote, urlsplit
import xml.etree.ElementTree as ET

MARKER = re.compile(r'<!-- diagram-maintenance\s*\n(.*?)\ndiagram-maintenance -->', re.S)
DESIGN = 'docs/design/'


def git(*args):
    return subprocess.check_output(['git', *args], text=True).strip()


def repository_path(root, value):
    if not isinstance(value, str) or not value or value.startswith('/'):
        raise ValueError('paths must be nonempty repository-relative strings')
    path = root / value
    if '..' in Path(value).parts or not path.resolve().is_relative_to(root.resolve()):
        raise ValueError(f'path escapes repository: {value}')
    return path


def check_record(root, body, changed):
    records = MARKER.findall(body)
    if len(records) != 1:
        raise ValueError('exactly one diagram-maintenance JSON record is required')
    record = json.loads(records[0])
    mappings = record.get('mappings')
    if not isinstance(mappings, list) or not mappings:
        raise ValueError('nonempty mappings required for every PR')
    validation = record.get('validation')
    if not isinstance(validation, str) or len(validation.strip()) < 30:
        raise ValueError('describe render and legibility validation')
    covered = set()
    for mapping in mappings:
        visual = mapping.get('visual')
        path = repository_path(root, visual)
        if not visual.startswith(DESIGN) or path.suffix != '.md' or visual not in changed or not path.is_file():
            raise ValueError(f'mapped design source must exist and change in this PR: {visual}')
        reason = mapping.get('reason')
        if not isinstance(reason, str) or len(reason.strip()) < 30:
            raise ValueError('explain the implementation comparison / invariant checked')
        sources = mapping.get('sources')
        if not isinstance(sources, list) or not sources:
            raise ValueError('each mapping needs changed source paths')
        for source in sources:
            repository_path(root, source)
            if source not in changed:
                raise ValueError(f'mapped implementation path did not change: {source}')
            covered.add(source)
    missing = {p for p in changed if not p.startswith(DESIGN)} - covered
    if missing:
        raise ValueError(f'unmapped changed paths: {sorted(missing)}')
    # A diagram-only PR must still describe the changed visual implementation.
    if not covered.intersection(changed):
        raise ValueError('mapping must cover changed implementation')


def check_document(root, path):
    text = path.read_text()
    # Design sources intentionally contain only supported UML visual fences,
    # plus ordinary shell command snippets in the index.
    in_fence = False
    language = None
    block = []
    for line in text.splitlines():
        match = re.match(r'^\s*(`{3,}|~{3,})(.*)$', line)
        if not match:
            if in_fence:
                block.append(line)
            continue
        if match[1] != '```' or line[:1].isspace():
            raise ValueError(f'{path}: use canonical unindented triple-backtick fences')
        if not in_fence:
            language = match[2].strip()
            if language not in {'plantuml', 'puml', 'sh'}:
                raise ValueError(f'{path}: unsupported fence {language!r}')
            in_fence, block = True, []
        else:
            if match[2].strip():
                raise ValueError(f'{path}: nested/malformed fence')
            if language in {'plantuml', 'puml'}:
                source = '\n'.join(block).strip()
                if not source.startswith('@startuml\n') or not source.endswith('\n@enduml') or source.count('@startuml') != 1 or source.count('@enduml') != 1:
                    raise ValueError(f'{path}: expected exactly one complete UML diagram')
                if source.count('{') != source.count('}'):
                    raise ValueError(f'{path}: unbalanced UML braces')
                if re.search(r'^\s*!(?:include|includeurl)', source, re.M):
                    raise ValueError(f'{path}: inline the diagram source; external includes are unsupported here')
            in_fence = False
    if in_fence:
        raise ValueError(f'{path}: unclosed fence')
    for target in re.findall(r'\]\(([^\s)]+)\)', text):
        parsed = urlsplit(target)
        if parsed.scheme in {'http', 'https', 'mailto'} or target.startswith('#'):
            continue
        linked = (path.parent / unquote(parsed.path)).resolve()
        if not linked.is_relative_to(root.resolve()) or not linked.exists():
            raise ValueError(f'{path}: broken/escaping local link {target}')
        if parsed.fragment and linked.suffix == '.md':
            headings = re.findall(r'^#+\s+(.+)$', linked.read_text(), re.M)
            anchors = {re.sub(r'[^\w\- ]', '', h.lower()).replace(' ', '-') for h in headings}
            if unquote(parsed.fragment) not in anchors:
                raise ValueError(f'{path}: missing heading {target}')
    return len(re.findall(r'^```(?:plantuml|puml)\s*$', text, re.M))


def check_svg(path):
    tree = ET.parse(path)
    if not any(e.tag.rsplit('}', 1)[-1] in {'text', 'foreignObject'} and ''.join(e.itertext()).strip() for e in tree.iter()):
        raise ValueError(f'{path}: renderer produced an empty/unlabelled diagram')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base', help='PR base ref; checked against merge-base')
    parser.add_argument('--body', type=Path, help='Markdown PR body')
    parser.add_argument('--event', type=Path, help='GitHub pull_request event JSON')
    parser.add_argument('--render', type=Path, help='external output directory; requires documd 6.0.0')
    args = parser.parse_args()
    root = Path(git('rev-parse', '--show-toplevel'))
    os.chdir(root)
    if args.event:
        event = json.loads(args.event.read_text())
        body = event['pull_request'].get('body') or ''
        base = event['pull_request']['base']['sha']
    elif args.base and args.body:
        body, base = args.body.read_text(), args.base
    else:
        parser.error('use --event or both --base and --body')
    common = git('merge-base', base, 'HEAD')
    changed = set(filter(None, subprocess.check_output(['git', 'diff', '--name-only', '-z', common, 'HEAD'], text=True).split('\0')))
    check_record(root, body, changed)
    diagrams = 0
    for path in sorted((root / DESIGN).glob('*.md')):
        count = check_document(root, path)
        diagrams += count
        if args.render and count:
            output = args.render.resolve()
            if output.is_relative_to(root):
                raise ValueError('render output must be outside the source checkout')
            version = subprocess.check_output(['documd', '--version'], text=True)
            if 'v6.0.0' not in version:
                raise ValueError(f'expected documd 6.0.0, got {version.strip()}')
            output.mkdir(parents=True, exist_ok=True)
            subprocess.run(['documd', str(path), '--assets', str(output), '--format', 'svg'], check=True)
            exports = list(output.glob(f'{path.stem}-*-plantuml.svg'))
            if len(exports) != count:
                raise ValueError(f'{path}: expected {count} SVG exports, found {len(exports)}')
            for exported in exports:
                check_svg(exported)
    if diagrams == 0:
        raise ValueError('at least one maintained diagram is required')
    print(f'Design contract passed: {len(changed)} changed paths, {diagrams} diagrams; semantic review still required.')


if __name__ == '__main__':
    try:
        main()
    except (ValueError, KeyError, TypeError, OSError, subprocess.CalledProcessError) as exc:
        sys.exit(f'Design maintenance failed: {exc}')
