"""Mutation-check release guards in an isolated copy, using only Python's standard library."""
import ast
import copy
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
source = (ROOT / "scripts/publish_release.py").read_text()
tree = ast.parse(source)
mutations = []
for index, node in enumerate(ast.walk(tree)):
    if isinstance(node, ast.If):
        for value in [True, False]:
            mutations.append((index, "test", ast.Constant(value), f"line {node.lineno}: guard={value}"))
    if isinstance(node, ast.comprehension) and node.ifs:
        for value in [True, False]:
            mutations.append((index, "ifs", [ast.Constant(value)], f"comprehension {index}: filter={value}"))
    if isinstance(node, ast.Return):
        mutations.append((index, "value", ast.Constant(None), f"line {node.lineno}: return None"))
    if isinstance(node, ast.keyword) and node.arg == "check":
        mutations.append((index, "value", ast.Constant(False), f"line {node.value.lineno}: unchecked subprocess"))
    if isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute) and node.func.attr == "exit":
        mutations.append((index, "args", [ast.Constant(0)], f"line {node.lineno}: successful error exit"))
    if isinstance(node, ast.BoolOp):
        mutations.append((index, "op", ast.And() if isinstance(node.op, ast.Or) else ast.Or(), f"line {node.lineno}: boolean operator"))
    if isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute) and node.func.attr == "fullmatch":
        func = copy.deepcopy(node.func); func.attr = "search"
        mutations.append((index, "func", func, f"line {node.lineno}: regex search"))
    if isinstance(node, ast.Constant) and isinstance(node.value, str) and "[1-9][0-9]*" in node.value:
        for part in range(3):
            bits = node.value.split("(0|[1-9][0-9]*)")
            pattern = ""
            for n, bit in enumerate(bits):
                pattern += bit
                if n < 3:
                    pattern += "[0-9]+" if n == part else "(0|[1-9][0-9]*)"
            mutations.append((index, "value", pattern, f"line {node.lineno}: leading zero component {part}"))

with tempfile.TemporaryDirectory(prefix="release-mutants-") as temporary:
    root = Path(temporary)
    for folder in ["scripts", "tests", ".github"]:
        shutil.copytree(ROOT / folder, root / folder, ignore=shutil.ignore_patterns("__pycache__"))
    for file in ["Cargo.toml", "Cargo.lock", "herdr-plugin.toml"]:
        shutil.copy2(ROOT / file, root / file)
    def run():
        return subprocess.run([sys.executable, "-B", "-m", "unittest", "discover", "-s", "tests", "-p", "test_release.py"], cwd=root, capture_output=True, text=True, timeout=15)
    baseline = run()
    if baseline.returncode:
        sys.exit(baseline.stdout + baseline.stderr)
    survived = []
    for index, field, value, name in mutations:
        changed = copy.deepcopy(tree)
        setattr(list(ast.walk(changed))[index], field, copy.deepcopy(value))
        path = root / "scripts/publish_release.py"
        path.write_text(ast.unparse(ast.fix_missing_locations(changed)))
        result = run()
        if result.returncode == 0:
            survived.append(name)
        print(("SURVIVED " if result.returncode == 0 else "caught ") + name)
    if survived:
        sys.exit(f"{len(survived)} surviving mutations: {survived}")
    print(f"All {len(mutations)} release mutations caught")
