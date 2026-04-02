"""Copy .well-known to build output and generate agent-skills/index.json."""

import hashlib
import json
import logging
import re
import shutil
from pathlib import Path

import yaml

log = logging.getLogger("mkdocs.hooks.well_known")

SCHEMA = "https://schemas.agentskills.io/discovery/0.2.0/schema.json"


def parse_frontmatter(text):
    m = re.match(r"^---\r?\n(.+?)\r?\n---", text, re.DOTALL)
    if not m:
        return {}
    return yaml.safe_load(m.group(1)) or {}


def generate_skills_index(skills_dir):
    skills = []
    for skill_md in sorted(skills_dir.glob("*/SKILL.md")):
        raw = skill_md.read_bytes()
        fm = parse_frontmatter(raw.decode("utf-8"))
        if "name" not in fm or "description" not in fm:
            log.warning("Skipping %s: missing 'name' or 'description' in frontmatter", skill_md)
            continue
        url = "/.well-known/agent-skills/" + skill_md.parent.name + "/SKILL.md"
        skills.append({
            "name": fm["name"],
            "type": "skill-md",
            "description": fm["description"],
            "url": url,
            "digest": "sha256:" + hashlib.sha256(raw).hexdigest(),
        })
    return {"$schema": SCHEMA, "skills": skills}


def on_post_build(config, **kwargs):
    src = Path(config["docs_dir"]) / ".well-known"
    dst = Path(config["site_dir"]) / ".well-known"
    if not src.is_dir():
        return
    shutil.copytree(src, dst, dirs_exist_ok=True)

    skills_dir = dst / "agent-skills"
    if skills_dir.is_dir():
        index = generate_skills_index(skills_dir)
        (skills_dir / "index.json").write_text(
            json.dumps(index, indent=2) + "\n"
        )
