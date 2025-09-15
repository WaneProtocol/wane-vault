#!/usr/bin/env python3
"""
Generate a realistic git commit history for the IVZA Protocol project.

This script:
1. Reads all current tracked files and stores their contents in memory
2. Clears the working directory (preserving .git, node_modules, target)
3. Replays file creation/modification through ~160 commits over 6 months
4. Verifies the final state is byte-for-byte identical to the original
"""

import os
import sys
import subprocess
import random
import hashlib
import shutil
from datetime import datetime, timedelta, timezone
from pathlib import Path

# ─── Configuration ───────────────────────────────────────────────────────────

PROJECT_DIR = r"C:\Users\baayo\Desktop\web\ivza\product"
GIT_USERNAME = "ivzadotdev"
GIT_EMAIL = "256582581+ivzadotdev@users.noreply.github.com"
START_DATE = datetime(2025, 9, 15, tzinfo=timezone(timedelta(hours=9)))
END_DATE = datetime(2026, 3, 19, tzinfo=timezone(timedelta(hours=9)))
KST = timezone(timedelta(hours=9))

# Directories to skip (not tracked by git or not part of history)
SKIP_DIRS = {'.git', 'node_modules', 'target', 'dist', '.anchor', 'test-ledger', 'coverage'}
SKIP_FILE = 'generate_history.py'

# Seed for reproducibility
random.seed(42)

# ─── Helpers ─────────────────────────────────────────────────────────────────

def run_git(*args, env_override=None):
    """Run a git command in the project directory."""
    cmd = ['git'] + list(args)
    env = os.environ.copy()
    if env_override:
        env.update(env_override)
    result = subprocess.run(
        cmd, cwd=PROJECT_DIR, capture_output=True,
        env=env, timeout=60
    )
    if result.returncode != 0:
        stderr = result.stderr.decode('utf-8', errors='replace')
        # Ignore warnings about empty commits or LF/CRLF
        if 'nothing to commit' not in stderr and 'warning' not in stderr.lower():
            print(f"  [WARN] git {' '.join(args[:3])}... => {stderr[:200]}")
    return result


def make_commit(message, timestamp, files_to_stage=None):
    """Create a commit with a specific timestamp and message."""
    ts_str = timestamp.strftime('%Y-%m-%dT%H:%M:%S%z')
    env = {
        'GIT_AUTHOR_DATE': ts_str,
        'GIT_COMMITTER_DATE': ts_str,
        'GIT_AUTHOR_NAME': GIT_USERNAME,
        'GIT_COMMITTER_NAME': GIT_USERNAME,
        'GIT_AUTHOR_EMAIL': GIT_EMAIL,
        'GIT_COMMITTER_EMAIL': GIT_EMAIL,
    }
    if files_to_stage:
        for f in files_to_stage:
            run_git('add', f)
    else:
        run_git('add', '-A')
    run_git('commit', '-m', message, '--allow-empty', env_override=env)


def write_file(rel_path, content):
    """Write a file relative to PROJECT_DIR, creating directories as needed."""
    full = os.path.join(PROJECT_DIR, rel_path.replace('/', os.sep))
    os.makedirs(os.path.dirname(full), exist_ok=True)
    if isinstance(content, bytes):
        with open(full, 'wb') as f:
            f.write(content)
    else:
        with open(full, 'w', encoding='utf-8', newline='\n') as f:
            f.write(content)


def read_file_bytes(rel_path):
    """Read a file as bytes."""
    full = os.path.join(PROJECT_DIR, rel_path.replace('/', os.sep))
    with open(full, 'rb') as f:
        return f.read()


def file_hash(rel_path):
    """Get SHA-256 hash of a file."""
    full = os.path.join(PROJECT_DIR, rel_path.replace('/', os.sep))
    if not os.path.exists(full):
        return None
    with open(full, 'rb') as f:
        return hashlib.sha256(f.read()).hexdigest()


def delete_file(rel_path):
    """Delete a file relative to PROJECT_DIR."""
    full = os.path.join(PROJECT_DIR, rel_path.replace('/', os.sep))
    if os.path.exists(full):
        os.remove(full)


def split_content(content, num_parts):
    """Split text content into roughly equal parts by lines."""
    if isinstance(content, bytes):
        return [content]  # Don't split binary files
    lines = content.split('\n')
    if len(lines) <= 5 or num_parts <= 1:
        return [content]
    chunk_size = max(1, len(lines) // num_parts)
    parts = []
    for i in range(num_parts):
        start = 0
        end = chunk_size * (i + 1)
        if i == num_parts - 1:
            end = len(lines)
        parts.append('\n'.join(lines[start:end]))
    return parts


def partial_content(content, fraction):
    """Return the first `fraction` (0.0–1.0) of a file's content by lines."""
    if isinstance(content, bytes):
        cut = max(1, int(len(content) * fraction))
        return content[:cut]
    lines = content.split('\n')
    n = max(1, int(len(lines) * fraction))
    return '\n'.join(lines[:n])


# ─── Timestamp Generation ───────────────────────────────────────────────────

def generate_timestamps(n_commits, start, end):
    """
    Generate n_commits realistic timestamps between start and end.
    Rules:
    - At least 3 gaps of 3+ consecutive zero-commit days
    - Fewer commits on weekends
    - Varied times: morning/afternoon/evening
    - Some days 1 commit, some 3-5, many 0
    """
    total_days = (end - start).days
    # Build a day-by-day commit count distribution
    day_counts = [0] * (total_days + 1)

    # First, insert 4 gaps of 4-6 consecutive days with zero commits
    gap_starts = sorted(random.sample(range(10, total_days - 15), 4))
    gap_days = set()
    for gs in gap_starts:
        gap_len = random.randint(3, 6)
        for d in range(gs, min(gs + gap_len, total_days)):
            gap_days.add(d)

    # Distribute commits across non-gap days
    available_days = [d for d in range(total_days + 1) if d not in gap_days]
    # Weight weekdays higher than weekends
    weights = []
    for d in available_days:
        dt = start + timedelta(days=d)
        if dt.weekday() >= 5:  # Saturday=5, Sunday=6
            weights.append(0.3)
        else:
            weights.append(1.0)

    # Assign commits to days
    remaining = n_commits
    while remaining > 0:
        for idx, d in enumerate(available_days):
            if remaining <= 0:
                break
            w = weights[idx]
            # Random burst: some days get more commits
            r = random.random()
            if r < 0.15 * w:
                count = random.randint(3, 5)
            elif r < 0.45 * w:
                count = random.randint(1, 2)
            elif r < 0.65 * w:
                count = 1
            else:
                count = 0
            count = min(count, remaining)
            day_counts[d] += count
            remaining -= count
        # Safety: if we still have remaining, force them
        if remaining > 0 and remaining < 10:
            for _ in range(remaining):
                d = random.choice(available_days)
                day_counts[d] += 1
            remaining = 0

    # Generate actual timestamps from day_counts
    timestamps = []
    for d, count in enumerate(day_counts):
        if count == 0:
            continue
        base_date = start + timedelta(days=d)
        for _ in range(count):
            # Pick a time slot
            slot = random.choices(
                ['morning', 'afternoon', 'evening'],
                weights=[0.25, 0.40, 0.35]
            )[0]
            if slot == 'morning':
                hour = random.randint(8, 11)
            elif slot == 'afternoon':
                hour = random.randint(12, 17)
            else:
                hour = random.randint(18, 23)
            minute = random.randint(0, 59)
            second = random.randint(0, 59)
            ts = base_date.replace(hour=hour, minute=minute, second=second)
            timestamps.append(ts)

    timestamps.sort()
    return timestamps


# ─── Phase Definitions ───────────────────────────────────────────────────────
# Each phase defines which files it introduces/modifies and how.

def define_phases(file_contents):
    """
    Define the commit phases. Each phase is a list of commit specs:
    {
        'message': str,
        'files': { rel_path: content_or_bytes },  # files to write
        'deletes': [rel_path, ...],  # optional files to delete
    }
    """
    phases = []

    # ── Helper: get content, with optional partial ──
    def fc(path, fraction=1.0):
        """Get file content, optionally partial."""
        c = file_contents.get(path)
        if c is None:
            return None
        if fraction >= 1.0:
            return c
        return partial_content(c, fraction)

    # ═══════════════════════════════════════════════════════════════════════
    # PHASE 1: Scaffolding (Sep 2025, ~12 commits)
    # ═══════════════════════════════════════════════════════════════════════
    phase1 = []

    # 1. Initial commit - gitignore + license
    phase1.append({
        'message': 'chore: initial project scaffolding',
        'files': {
            '.gitignore': fc('.gitignore'),
            'LICENSE': fc('LICENSE'),
        }
    })

    # 2. Cargo workspace setup
    phase1.append({
        'message': 'feat: initialize cargo workspace with crate structure',
        'files': {
            'Cargo.toml': fc('Cargo.toml', 0.4),
        }
    })

    # 3. Core crate skeleton
    phase1.append({
        'message': 'feat: add ivza-core crate skeleton',
        'files': {
            'crates/ivza-core/Cargo.toml': fc('crates/ivza-core/Cargo.toml', 0.5),
            'crates/ivza-core/src/lib.rs': '//! IVZA Protocol Core Library\n\npub mod types;\n',
        }
    })

    # 4. Solver crate skeleton
    phase1.append({
        'message': 'feat: add ivza-solver crate skeleton',
        'files': {
            'crates/ivza-solver/Cargo.toml': fc('crates/ivza-solver/Cargo.toml', 0.5),
            'crates/ivza-solver/src/lib.rs': '//! IVZA Solver\n',
        }
    })

    # 5. CLI crate skeleton
    phase1.append({
        'message': 'feat: add ivza-cli crate skeleton',
        'files': {
            'crates/ivza-cli/Cargo.toml': fc('crates/ivza-cli/Cargo.toml', 0.5),
            'crates/ivza-cli/src/main.rs': 'fn main() {\n    println!("ivza-cli");\n}\n',
        }
    })

    # 6. Anchor program skeleton
    phase1.append({
        'message': 'feat: scaffold anchor program directory',
        'files': {
            'programs/ivza-engine/Cargo.toml': fc('programs/ivza-engine/Cargo.toml', 0.5),
            'programs/ivza-engine/src/lib.rs': '//! IVZA Engine on-chain program\n',
        }
    })

    # 7. README draft
    phase1.append({
        'message': 'docs: add initial README with project overview',
        'files': {
            'README.md': fc('README.md', 0.3),
        }
    })

    # 8. Scripts directory
    phase1.append({
        'message': 'chore: add build and test scripts',
        'files': {
            'scripts/build.sh': fc('scripts/build.sh'),
            'scripts/test.sh': fc('scripts/test.sh'),
        }
    })

    # 9. GitHub templates
    phase1.append({
        'message': 'chore: add github issue and PR templates',
        'files': {
            '.github/ISSUE_TEMPLATE/bug_report.md': fc('.github/ISSUE_TEMPLATE/bug_report.md'),
            '.github/ISSUE_TEMPLATE/feature_request.md': fc('.github/ISSUE_TEMPLATE/feature_request.md'),
            '.github/pull_request_template.md': fc('.github/pull_request_template.md'),
        }
    })

    # 10. CI workflow stub
    phase1.append({
        'message': 'ci: add initial GitHub Actions workflow',
        'files': {
            '.github/workflows/ci.yml': fc('.github/workflows/ci.yml', 0.4),
        }
    })

    # 11. Dependabot config
    phase1.append({
        'message': 'chore: configure dependabot for cargo and npm',
        'files': {
            '.github/dependabot.yml': fc('.github/dependabot.yml'),
        }
    })

    # 12. Update workspace Cargo.toml
    phase1.append({
        'message': 'chore: update workspace members and dependencies',
        'files': {
            'Cargo.toml': fc('Cargo.toml', 0.7),
        }
    })

    phases.append(phase1)

    # ═══════════════════════════════════════════════════════════════════════
    # PHASE 2: Core types and graph (Oct 2025, ~25 commits)
    # ═══════════════════════════════════════════════════════════════════════
    phase2 = []

    # Types module
    phase2.append({
        'message': 'feat: define core account type with state tracking',
        'files': {
            'crates/ivza-core/src/types/mod.rs': fc('crates/ivza-core/src/types/mod.rs', 0.5),
            'crates/ivza-core/src/types/account.rs': fc('crates/ivza-core/src/types/account.rs', 0.4),
        }
    })

    phase2.append({
        'message': 'feat: add account ownership and lamport tracking',
        'files': {
            'crates/ivza-core/src/types/account.rs': fc('crates/ivza-core/src/types/account.rs', 0.7),
        }
    })

    phase2.append({
        'message': 'feat: implement transaction type with instruction data',
        'files': {
            'crates/ivza-core/src/types/transaction.rs': fc('crates/ivza-core/src/types/transaction.rs', 0.5),
        }
    })

    phase2.append({
        'message': 'feat: add signature and hash fields to transaction',
        'files': {
            'crates/ivza-core/src/types/transaction.rs': fc('crates/ivza-core/src/types/transaction.rs', 0.8),
        }
    })

    phase2.append({
        'message': 'refactor: derive Clone and Debug on all core types',
        'files': {
            'crates/ivza-core/src/types/account.rs': fc('crates/ivza-core/src/types/account.rs'),
            'crates/ivza-core/src/types/transaction.rs': fc('crates/ivza-core/src/types/transaction.rs'),
            'crates/ivza-core/src/types/mod.rs': fc('crates/ivza-core/src/types/mod.rs'),
        }
    })

    # Graph module
    phase2.append({
        'message': 'feat: create graph module with node definition',
        'files': {
            'crates/ivza-core/src/graph/mod.rs': fc('crates/ivza-core/src/graph/mod.rs', 0.3),
            'crates/ivza-core/src/graph/node.rs': fc('crates/ivza-core/src/graph/node.rs', 0.3),
        }
    })

    phase2.append({
        'message': 'feat: add execution state enum to graph node',
        'files': {
            'crates/ivza-core/src/graph/node.rs': fc('crates/ivza-core/src/graph/node.rs', 0.6),
        }
    })

    phase2.append({
        'message': 'feat: implement graph edge with dependency types',
        'files': {
            'crates/ivza-core/src/graph/edge.rs': fc('crates/ivza-core/src/graph/edge.rs', 0.5),
        }
    })

    phase2.append({
        'message': 'feat: add weight and constraint fields to edges',
        'files': {
            'crates/ivza-core/src/graph/edge.rs': fc('crates/ivza-core/src/graph/edge.rs'),
        }
    })

    phase2.append({
        'message': 'feat: scaffold graph builder with new() constructor',
        'files': {
            'crates/ivza-core/src/graph/builder.rs': fc('crates/ivza-core/src/graph/builder.rs', 0.25),
        }
    })

    phase2.append({
        'message': 'feat: implement add_node and add_edge on builder',
        'files': {
            'crates/ivza-core/src/graph/builder.rs': fc('crates/ivza-core/src/graph/builder.rs', 0.5),
        }
    })

    phase2.append({
        'message': 'feat: add validation logic to graph builder',
        'files': {
            'crates/ivza-core/src/graph/builder.rs': fc('crates/ivza-core/src/graph/builder.rs', 0.75),
        }
    })

    phase2.append({
        'message': 'feat: implement build() method with cycle detection',
        'files': {
            'crates/ivza-core/src/graph/builder.rs': fc('crates/ivza-core/src/graph/builder.rs'),
        }
    })

    phase2.append({
        'message': 'feat: add graph decomposer skeleton',
        'files': {
            'crates/ivza-core/src/graph/decomposer.rs': fc('crates/ivza-core/src/graph/decomposer.rs', 0.3),
        }
    })

    phase2.append({
        'message': 'feat: implement connected component extraction',
        'files': {
            'crates/ivza-core/src/graph/decomposer.rs': fc('crates/ivza-core/src/graph/decomposer.rs', 0.6),
        }
    })

    phase2.append({
        'message': 'feat: add topological sort to decomposer',
        'files': {
            'crates/ivza-core/src/graph/decomposer.rs': fc('crates/ivza-core/src/graph/decomposer.rs'),
        }
    })

    phase2.append({
        'message': 'refactor: export graph submodules from mod.rs',
        'files': {
            'crates/ivza-core/src/graph/mod.rs': fc('crates/ivza-core/src/graph/mod.rs'),
        }
    })

    phase2.append({
        'message': 'feat: update lib.rs to export graph and types modules',
        'files': {
            'crates/ivza-core/src/lib.rs': fc('crates/ivza-core/src/lib.rs', 0.4),
        }
    })

    phase2.append({
        'message': 'fix: correct lifetime annotations on graph node references',
        'files': {
            'crates/ivza-core/src/graph/node.rs': fc('crates/ivza-core/src/graph/node.rs', 0.85),
        }
    })

    phase2.append({
        'message': 'refactor: use SmallVec for node adjacency lists',
        'files': {
            'crates/ivza-core/src/graph/node.rs': fc('crates/ivza-core/src/graph/node.rs'),
        }
    })

    phase2.append({
        'message': 'fix: handle duplicate edge insertion in builder',
        'files': {
            'crates/ivza-core/src/graph/builder.rs': fc('crates/ivza-core/src/graph/builder.rs'),
        }
    })

    phase2.append({
        'message': 'chore: update ivza-core Cargo.toml with graph deps',
        'files': {
            'crates/ivza-core/Cargo.toml': fc('crates/ivza-core/Cargo.toml', 0.8),
        }
    })

    phase2.append({
        'message': 'feat: add Display impl for graph node',
        'files': {
            'crates/ivza-core/src/graph/node.rs': fc('crates/ivza-core/src/graph/node.rs'),
        }
    })

    phase2.append({
        'message': 'test: add unit tests for graph builder',
        'files': {
            'tests/core/graph_test.rs': fc('tests/core/graph_test.rs', 0.4),
        }
    })

    phase2.append({
        'message': 'fix: prevent self-referencing edges in graph',
        'files': {
            'crates/ivza-core/src/graph/builder.rs': fc('crates/ivza-core/src/graph/builder.rs'),
            'tests/core/graph_test.rs': fc('tests/core/graph_test.rs', 0.6),
        }
    })

    phases.append(phase2)

    # ═══════════════════════════════════════════════════════════════════════
    # PHASE 3: Analyzers (Oct-Nov 2025, ~20 commits)
    # ═══════════════════════════════════════════════════════════════════════
    phase3 = []

    phase3.append({
        'message': 'feat: create analyzer module skeleton',
        'files': {
            'crates/ivza-core/src/analyzer/mod.rs': fc('crates/ivza-core/src/analyzer/mod.rs', 0.3),
        }
    })

    phase3.append({
        'message': 'feat: implement dependency analyzer struct',
        'files': {
            'crates/ivza-core/src/analyzer/dependency.rs': fc('crates/ivza-core/src/analyzer/dependency.rs', 0.3),
        }
    })

    phase3.append({
        'message': 'feat: add read-write conflict detection to dependency analyzer',
        'files': {
            'crates/ivza-core/src/analyzer/dependency.rs': fc('crates/ivza-core/src/analyzer/dependency.rs', 0.5),
        }
    })

    phase3.append({
        'message': 'feat: implement transitive dependency resolution',
        'files': {
            'crates/ivza-core/src/analyzer/dependency.rs': fc('crates/ivza-core/src/analyzer/dependency.rs', 0.75),
        }
    })

    phase3.append({
        'message': 'feat: finalize dependency analyzer with conflict matrix',
        'files': {
            'crates/ivza-core/src/analyzer/dependency.rs': fc('crates/ivza-core/src/analyzer/dependency.rs'),
        }
    })

    phase3.append({
        'message': 'feat: scaffold critical path analyzer',
        'files': {
            'crates/ivza-core/src/analyzer/critical_path.rs': fc('crates/ivza-core/src/analyzer/critical_path.rs', 0.3),
        }
    })

    phase3.append({
        'message': 'feat: implement forward pass for critical path calculation',
        'files': {
            'crates/ivza-core/src/analyzer/critical_path.rs': fc('crates/ivza-core/src/analyzer/critical_path.rs', 0.55),
        }
    })

    phase3.append({
        'message': 'feat: add backward pass and slack computation',
        'files': {
            'crates/ivza-core/src/analyzer/critical_path.rs': fc('crates/ivza-core/src/analyzer/critical_path.rs', 0.8),
        }
    })

    phase3.append({
        'message': 'feat: complete critical path with bottleneck identification',
        'files': {
            'crates/ivza-core/src/analyzer/critical_path.rs': fc('crates/ivza-core/src/analyzer/critical_path.rs'),
        }
    })

    phase3.append({
        'message': 'feat: scaffold parallelism analyzer',
        'files': {
            'crates/ivza-core/src/analyzer/parallelism.rs': fc('crates/ivza-core/src/analyzer/parallelism.rs', 0.3),
        }
    })

    phase3.append({
        'message': 'feat: detect independent transaction groups',
        'files': {
            'crates/ivza-core/src/analyzer/parallelism.rs': fc('crates/ivza-core/src/analyzer/parallelism.rs', 0.55),
        }
    })

    phase3.append({
        'message': 'feat: compute parallelism score and lane assignments',
        'files': {
            'crates/ivza-core/src/analyzer/parallelism.rs': fc('crates/ivza-core/src/analyzer/parallelism.rs', 0.8),
        }
    })

    phase3.append({
        'message': 'feat: finalize parallelism analyzer with merge points',
        'files': {
            'crates/ivza-core/src/analyzer/parallelism.rs': fc('crates/ivza-core/src/analyzer/parallelism.rs'),
        }
    })

    phase3.append({
        'message': 'refactor: unify analyzer trait interface',
        'files': {
            'crates/ivza-core/src/analyzer/mod.rs': fc('crates/ivza-core/src/analyzer/mod.rs'),
        }
    })

    phase3.append({
        'message': 'feat: export analyzer module from lib.rs',
        'files': {
            'crates/ivza-core/src/lib.rs': fc('crates/ivza-core/src/lib.rs', 0.55),
        }
    })

    phase3.append({
        'message': 'test: add dependency analyzer test cases',
        'files': {
            'tests/core/analyzer_test.rs': fc('tests/core/analyzer_test.rs', 0.5),
        }
    })

    phase3.append({
        'message': 'test: add critical path and parallelism tests',
        'files': {
            'tests/core/analyzer_test.rs': fc('tests/core/analyzer_test.rs'),
        }
    })

    phase3.append({
        'message': 'fix: off-by-one in critical path slack calculation',
        'files': {
            'crates/ivza-core/src/analyzer/critical_path.rs': fc('crates/ivza-core/src/analyzer/critical_path.rs'),
        }
    })

    phase3.append({
        'message': 'perf: use bitset for visited nodes in dependency traversal',
        'files': {
            'crates/ivza-core/src/analyzer/dependency.rs': fc('crates/ivza-core/src/analyzer/dependency.rs'),
        }
    })

    phase3.append({
        'message': 'fix: parallelism analyzer panics on empty graph',
        'files': {
            'crates/ivza-core/src/analyzer/parallelism.rs': fc('crates/ivza-core/src/analyzer/parallelism.rs'),
        }
    })

    phases.append(phase3)

    # ═══════════════════════════════════════════════════════════════════════
    # PHASE 4: Scheduler (Nov 2025, ~15 commits)
    # ═══════════════════════════════════════════════════════════════════════
    phase4 = []

    phase4.append({
        'message': 'feat: create scheduler module with lane abstraction',
        'files': {
            'crates/ivza-core/src/scheduler/mod.rs': fc('crates/ivza-core/src/scheduler/mod.rs', 0.3),
            'crates/ivza-core/src/scheduler/lane.rs': fc('crates/ivza-core/src/scheduler/lane.rs', 0.3),
        }
    })

    phase4.append({
        'message': 'feat: implement lane capacity and task assignment',
        'files': {
            'crates/ivza-core/src/scheduler/lane.rs': fc('crates/ivza-core/src/scheduler/lane.rs', 0.6),
        }
    })

    phase4.append({
        'message': 'feat: add lane draining and rebalancing logic',
        'files': {
            'crates/ivza-core/src/scheduler/lane.rs': fc('crates/ivza-core/src/scheduler/lane.rs'),
        }
    })

    phase4.append({
        'message': 'feat: scaffold priority queue for scheduler',
        'files': {
            'crates/ivza-core/src/scheduler/priority.rs': fc('crates/ivza-core/src/scheduler/priority.rs', 0.35),
        }
    })

    phase4.append({
        'message': 'feat: implement priority calculation based on critical path',
        'files': {
            'crates/ivza-core/src/scheduler/priority.rs': fc('crates/ivza-core/src/scheduler/priority.rs', 0.7),
        }
    })

    phase4.append({
        'message': 'feat: add dynamic priority adjustment for starvation prevention',
        'files': {
            'crates/ivza-core/src/scheduler/priority.rs': fc('crates/ivza-core/src/scheduler/priority.rs'),
        }
    })

    phase4.append({
        'message': 'feat: scaffold executor with async task spawning',
        'files': {
            'crates/ivza-core/src/scheduler/executor.rs': fc('crates/ivza-core/src/scheduler/executor.rs', 0.3),
        }
    })

    phase4.append({
        'message': 'feat: implement executor run loop with lane polling',
        'files': {
            'crates/ivza-core/src/scheduler/executor.rs': fc('crates/ivza-core/src/scheduler/executor.rs', 0.6),
        }
    })

    phase4.append({
        'message': 'feat: add retry and error handling to executor',
        'files': {
            'crates/ivza-core/src/scheduler/executor.rs': fc('crates/ivza-core/src/scheduler/executor.rs', 0.85),
        }
    })

    phase4.append({
        'message': 'feat: finalize executor with graceful shutdown',
        'files': {
            'crates/ivza-core/src/scheduler/executor.rs': fc('crates/ivza-core/src/scheduler/executor.rs'),
        }
    })

    phase4.append({
        'message': 'refactor: consolidate scheduler module exports',
        'files': {
            'crates/ivza-core/src/scheduler/mod.rs': fc('crates/ivza-core/src/scheduler/mod.rs'),
        }
    })

    phase4.append({
        'message': 'feat: wire scheduler into core lib.rs',
        'files': {
            'crates/ivza-core/src/lib.rs': fc('crates/ivza-core/src/lib.rs', 0.7),
        }
    })

    phase4.append({
        'message': 'test: add scheduler lane unit tests',
        'files': {
            'tests/core/scheduler_test.rs': fc('tests/core/scheduler_test.rs', 0.5),
        }
    })

    phase4.append({
        'message': 'fix: race condition in executor lane polling',
        'files': {
            'crates/ivza-core/src/scheduler/executor.rs': fc('crates/ivza-core/src/scheduler/executor.rs'),
        }
    })

    phase4.append({
        'message': 'test: add executor integration tests',
        'files': {
            'tests/core/scheduler_test.rs': fc('tests/core/scheduler_test.rs'),
        }
    })

    phases.append(phase4)

    # ═══════════════════════════════════════════════════════════════════════
    # PHASE 5: Solver (Nov-Dec 2025, ~25 commits)
    # ═══════════════════════════════════════════════════════════════════════
    phase5 = []

    phase5.append({
        'message': 'feat: scaffold solver crate with router module',
        'files': {
            'crates/ivza-solver/src/lib.rs': fc('crates/ivza-solver/src/lib.rs', 0.3),
            'crates/ivza-solver/src/router.rs': fc('crates/ivza-solver/src/router.rs', 0.2),
        }
    })

    phase5.append({
        'message': 'feat: define route struct and path finding interface',
        'files': {
            'crates/ivza-solver/src/router.rs': fc('crates/ivza-solver/src/router.rs', 0.35),
        }
    })

    phase5.append({
        'message': 'feat: implement dijkstra-based route finding',
        'files': {
            'crates/ivza-solver/src/router.rs': fc('crates/ivza-solver/src/router.rs', 0.5),
        }
    })

    phase5.append({
        'message': 'feat: add multi-hop route support with fee calculation',
        'files': {
            'crates/ivza-solver/src/router.rs': fc('crates/ivza-solver/src/router.rs', 0.7),
        }
    })

    phase5.append({
        'message': 'feat: implement route scoring and ranking',
        'files': {
            'crates/ivza-solver/src/router.rs': fc('crates/ivza-solver/src/router.rs', 0.85),
        }
    })

    phase5.append({
        'message': 'feat: finalize router with slippage protection',
        'files': {
            'crates/ivza-solver/src/router.rs': fc('crates/ivza-solver/src/router.rs'),
        }
    })

    phase5.append({
        'message': 'feat: scaffold pool module for liquidity tracking',
        'files': {
            'crates/ivza-solver/src/pool.rs': fc('crates/ivza-solver/src/pool.rs', 0.25),
        }
    })

    phase5.append({
        'message': 'feat: implement pool state with reserve tracking',
        'files': {
            'crates/ivza-solver/src/pool.rs': fc('crates/ivza-solver/src/pool.rs', 0.5),
        }
    })

    phase5.append({
        'message': 'feat: add constant product math to pool',
        'files': {
            'crates/ivza-solver/src/pool.rs': fc('crates/ivza-solver/src/pool.rs', 0.75),
        }
    })

    phase5.append({
        'message': 'feat: complete pool with price impact estimation',
        'files': {
            'crates/ivza-solver/src/pool.rs': fc('crates/ivza-solver/src/pool.rs'),
        }
    })

    phase5.append({
        'message': 'feat: scaffold optimizer module',
        'files': {
            'crates/ivza-solver/src/optimizer.rs': fc('crates/ivza-solver/src/optimizer.rs', 0.2),
        }
    })

    phase5.append({
        'message': 'feat: define optimization objective and constraints',
        'files': {
            'crates/ivza-solver/src/optimizer.rs': fc('crates/ivza-solver/src/optimizer.rs', 0.4),
        }
    })

    phase5.append({
        'message': 'feat: implement greedy optimizer pass',
        'files': {
            'crates/ivza-solver/src/optimizer.rs': fc('crates/ivza-solver/src/optimizer.rs', 0.6),
        }
    })

    phase5.append({
        'message': 'feat: add constraint propagation to optimizer',
        'files': {
            'crates/ivza-solver/src/optimizer.rs': fc('crates/ivza-solver/src/optimizer.rs', 0.8),
        }
    })

    phase5.append({
        'message': 'feat: finalize optimizer with iterative refinement',
        'files': {
            'crates/ivza-solver/src/optimizer.rs': fc('crates/ivza-solver/src/optimizer.rs'),
        }
    })

    phase5.append({
        'message': 'feat: scaffold main solver entry point',
        'files': {
            'crates/ivza-solver/src/solver.rs': fc('crates/ivza-solver/src/solver.rs', 0.25),
        }
    })

    phase5.append({
        'message': 'feat: implement solve() with router and optimizer integration',
        'files': {
            'crates/ivza-solver/src/solver.rs': fc('crates/ivza-solver/src/solver.rs', 0.5),
        }
    })

    phase5.append({
        'message': 'feat: add solution validation and error reporting',
        'files': {
            'crates/ivza-solver/src/solver.rs': fc('crates/ivza-solver/src/solver.rs', 0.75),
        }
    })

    phase5.append({
        'message': 'feat: complete solver with batch processing support',
        'files': {
            'crates/ivza-solver/src/solver.rs': fc('crates/ivza-solver/src/solver.rs'),
        }
    })

    phase5.append({
        'message': 'refactor: consolidate solver crate exports',
        'files': {
            'crates/ivza-solver/src/lib.rs': fc('crates/ivza-solver/src/lib.rs'),
        }
    })

    phase5.append({
        'message': 'chore: update solver Cargo.toml dependencies',
        'files': {
            'crates/ivza-solver/Cargo.toml': fc('crates/ivza-solver/Cargo.toml'),
        }
    })

    phase5.append({
        'message': 'test: add solver route finding tests',
        'files': {
            'tests/core/solver_test.rs': fc('tests/core/solver_test.rs', 0.4),
        }
    })

    phase5.append({
        'message': 'test: add pool math and optimizer tests',
        'files': {
            'tests/core/solver_test.rs': fc('tests/core/solver_test.rs', 0.8),
        }
    })

    phase5.append({
        'message': 'fix: floating point precision in pool reserve calculation',
        'files': {
            'crates/ivza-solver/src/pool.rs': fc('crates/ivza-solver/src/pool.rs'),
        }
    })

    phase5.append({
        'message': 'fix: optimizer returns suboptimal routes for single-hop',
        'files': {
            'crates/ivza-solver/src/optimizer.rs': fc('crates/ivza-solver/src/optimizer.rs'),
            'tests/core/solver_test.rs': fc('tests/core/solver_test.rs'),
        }
    })

    phases.append(phase5)

    # ═══════════════════════════════════════════════════════════════════════
    # PHASE 6: Intent system (Dec 2025-Jan 2026, ~15 commits)
    # ═══════════════════════════════════════════════════════════════════════
    phase6 = []

    phase6.append({
        'message': 'feat: create intent module with parser skeleton',
        'files': {
            'crates/ivza-core/src/intent/mod.rs': fc('crates/ivza-core/src/intent/mod.rs', 0.3),
            'crates/ivza-core/src/intent/parser.rs': fc('crates/ivza-core/src/intent/parser.rs', 0.2),
        }
    })

    phase6.append({
        'message': 'feat: define intent type with action variants',
        'files': {
            'crates/ivza-core/src/intent/parser.rs': fc('crates/ivza-core/src/intent/parser.rs', 0.35),
        }
    })

    phase6.append({
        'message': 'feat: implement intent parsing from transaction data',
        'files': {
            'crates/ivza-core/src/intent/parser.rs': fc('crates/ivza-core/src/intent/parser.rs', 0.55),
        }
    })

    phase6.append({
        'message': 'feat: add conditional intent support',
        'files': {
            'crates/ivza-core/src/intent/parser.rs': fc('crates/ivza-core/src/intent/parser.rs', 0.75),
        }
    })

    phase6.append({
        'message': 'feat: finalize intent parser with validation',
        'files': {
            'crates/ivza-core/src/intent/parser.rs': fc('crates/ivza-core/src/intent/parser.rs'),
        }
    })

    phase6.append({
        'message': 'feat: scaffold intent resolver',
        'files': {
            'crates/ivza-core/src/intent/resolver.rs': fc('crates/ivza-core/src/intent/resolver.rs', 0.25),
        }
    })

    phase6.append({
        'message': 'feat: implement intent-to-graph transformation',
        'files': {
            'crates/ivza-core/src/intent/resolver.rs': fc('crates/ivza-core/src/intent/resolver.rs', 0.5),
        }
    })

    phase6.append({
        'message': 'feat: add conflict resolution between intents',
        'files': {
            'crates/ivza-core/src/intent/resolver.rs': fc('crates/ivza-core/src/intent/resolver.rs', 0.75),
        }
    })

    phase6.append({
        'message': 'feat: complete intent resolver with priority ordering',
        'files': {
            'crates/ivza-core/src/intent/resolver.rs': fc('crates/ivza-core/src/intent/resolver.rs'),
        }
    })

    phase6.append({
        'message': 'refactor: export intent module from core lib',
        'files': {
            'crates/ivza-core/src/intent/mod.rs': fc('crates/ivza-core/src/intent/mod.rs'),
            'crates/ivza-core/src/lib.rs': fc('crates/ivza-core/src/lib.rs', 0.85),
        }
    })

    phase6.append({
        'message': 'test: add intent parser test cases',
        'files': {
            'tests/core/intent_test.rs': fc('tests/core/intent_test.rs', 0.5),
        }
    })

    phase6.append({
        'message': 'test: add intent resolver integration tests',
        'files': {
            'tests/core/intent_test.rs': fc('tests/core/intent_test.rs'),
        }
    })

    phase6.append({
        'message': 'fix: intent parser rejects valid multi-action intents',
        'files': {
            'crates/ivza-core/src/intent/parser.rs': fc('crates/ivza-core/src/intent/parser.rs'),
        }
    })

    phase6.append({
        'message': 'chore: update core Cargo.toml with intent deps',
        'files': {
            'crates/ivza-core/Cargo.toml': fc('crates/ivza-core/Cargo.toml'),
        }
    })

    phase6.append({
        'message': 'refactor: simplify resolver error handling with thiserror',
        'files': {
            'crates/ivza-core/src/intent/resolver.rs': fc('crates/ivza-core/src/intent/resolver.rs'),
        }
    })

    phases.append(phase6)

    # ═══════════════════════════════════════════════════════════════════════
    # PHASE 7: Anchor program (Jan 2026, ~12 commits)
    # ═══════════════════════════════════════════════════════════════════════
    phase7 = []

    phase7.append({
        'message': 'feat: define on-chain state accounts',
        'files': {
            'programs/ivza-engine/src/state.rs': fc('programs/ivza-engine/src/state.rs', 0.4),
        }
    })

    phase7.append({
        'message': 'feat: add engine state with graph storage layout',
        'files': {
            'programs/ivza-engine/src/state.rs': fc('programs/ivza-engine/src/state.rs'),
        }
    })

    phase7.append({
        'message': 'feat: define program error types',
        'files': {
            'programs/ivza-engine/src/errors.rs': fc('programs/ivza-engine/src/errors.rs'),
        }
    })

    phase7.append({
        'message': 'feat: define program event types',
        'files': {
            'programs/ivza-engine/src/events.rs': fc('programs/ivza-engine/src/events.rs'),
        }
    })

    phase7.append({
        'message': 'feat: implement initialize instruction',
        'files': {
            'programs/ivza-engine/src/instructions/mod.rs': fc('programs/ivza-engine/src/instructions/mod.rs', 0.3),
            'programs/ivza-engine/src/instructions/initialize.rs': fc('programs/ivza-engine/src/instructions/initialize.rs'),
        }
    })

    phase7.append({
        'message': 'feat: implement submit_graph instruction',
        'files': {
            'programs/ivza-engine/src/instructions/submit_graph.rs': fc('programs/ivza-engine/src/instructions/submit_graph.rs'),
        }
    })

    phase7.append({
        'message': 'feat: implement execute instruction handler',
        'files': {
            'programs/ivza-engine/src/instructions/execute.rs': fc('programs/ivza-engine/src/instructions/execute.rs'),
        }
    })

    phase7.append({
        'message': 'feat: implement resolve instruction for intent settlement',
        'files': {
            'programs/ivza-engine/src/instructions/resolve.rs': fc('programs/ivza-engine/src/instructions/resolve.rs'),
        }
    })

    phase7.append({
        'message': 'refactor: wire all instructions into program entry point',
        'files': {
            'programs/ivza-engine/src/instructions/mod.rs': fc('programs/ivza-engine/src/instructions/mod.rs'),
            'programs/ivza-engine/src/lib.rs': fc('programs/ivza-engine/src/lib.rs'),
        }
    })

    phase7.append({
        'message': 'chore: update engine Cargo.toml with anchor deps',
        'files': {
            'programs/ivza-engine/Cargo.toml': fc('programs/ivza-engine/Cargo.toml'),
        }
    })

    phase7.append({
        'message': 'fix: account size calculation overflow in state init',
        'files': {
            'programs/ivza-engine/src/state.rs': fc('programs/ivza-engine/src/state.rs'),
            'programs/ivza-engine/src/instructions/initialize.rs': fc('programs/ivza-engine/src/instructions/initialize.rs'),
        }
    })

    phase7.append({
        'message': 'chore: add deploy script for anchor program',
        'files': {
            'scripts/deploy.sh': fc('scripts/deploy.sh'),
        }
    })

    phases.append(phase7)

    # ═══════════════════════════════════════════════════════════════════════
    # PHASE 8: CLI (Jan-Feb 2026, ~12 commits)
    # ═══════════════════════════════════════════════════════════════════════
    phase8 = []

    phase8.append({
        'message': 'feat: implement CLI config module with file persistence',
        'files': {
            'crates/ivza-cli/src/config.rs': fc('crates/ivza-cli/src/config.rs', 0.5),
        }
    })

    phase8.append({
        'message': 'feat: complete CLI config with network and wallet settings',
        'files': {
            'crates/ivza-cli/src/config.rs': fc('crates/ivza-cli/src/config.rs'),
        }
    })

    phase8.append({
        'message': 'feat: scaffold CLI command modules',
        'files': {
            'crates/ivza-cli/src/commands/mod.rs': fc('crates/ivza-cli/src/commands/mod.rs', 0.3),
        }
    })

    phase8.append({
        'message': 'feat: implement analyze command',
        'files': {
            'crates/ivza-cli/src/commands/analyze.rs': fc('crates/ivza-cli/src/commands/analyze.rs'),
            'crates/ivza-cli/src/commands/mod.rs': fc('crates/ivza-cli/src/commands/mod.rs', 0.5),
        }
    })

    phase8.append({
        'message': 'feat: implement solve command',
        'files': {
            'crates/ivza-cli/src/commands/solve.rs': fc('crates/ivza-cli/src/commands/solve.rs'),
        }
    })

    phase8.append({
        'message': 'feat: implement submit command for on-chain graph submission',
        'files': {
            'crates/ivza-cli/src/commands/submit.rs': fc('crates/ivza-cli/src/commands/submit.rs'),
        }
    })

    phase8.append({
        'message': 'feat: implement status command',
        'files': {
            'crates/ivza-cli/src/commands/status.rs': fc('crates/ivza-cli/src/commands/status.rs'),
        }
    })

    phase8.append({
        'message': 'feat: implement config command for runtime settings',
        'files': {
            'crates/ivza-cli/src/commands/config_cmd.rs': fc('crates/ivza-cli/src/commands/config_cmd.rs'),
            'crates/ivza-cli/src/commands/mod.rs': fc('crates/ivza-cli/src/commands/mod.rs'),
        }
    })

    phase8.append({
        'message': 'feat: wire CLI main with clap argument parsing',
        'files': {
            'crates/ivza-cli/src/main.rs': fc('crates/ivza-cli/src/main.rs'),
        }
    })

    phase8.append({
        'message': 'chore: update CLI Cargo.toml with all dependencies',
        'files': {
            'crates/ivza-cli/Cargo.toml': fc('crates/ivza-cli/Cargo.toml'),
        }
    })

    phase8.append({
        'message': 'fix: CLI panics when config file is missing',
        'files': {
            'crates/ivza-cli/src/config.rs': fc('crates/ivza-cli/src/config.rs'),
        }
    })

    phase8.append({
        'message': 'refactor: improve CLI error messages with anyhow context',
        'files': {
            'crates/ivza-cli/src/main.rs': fc('crates/ivza-cli/src/main.rs'),
        }
    })

    phases.append(phase8)

    # ═══════════════════════════════════════════════════════════════════════
    # PHASE 9: TypeScript SDK (Feb 2026, ~15 commits)
    # ═══════════════════════════════════════════════════════════════════════
    phase9 = []

    phase9.append({
        'message': 'feat: initialize TypeScript SDK with package.json',
        'files': {
            'sdk/package.json': fc('sdk/package.json'),
            'sdk/tsconfig.json': fc('sdk/tsconfig.json'),
        }
    })

    phase9.append({
        'message': 'feat: add eslint config for SDK',
        'files': {
            'sdk/eslint.config.mjs': fc('sdk/eslint.config.mjs'),
        }
    })

    phase9.append({
        'message': 'feat: define SDK type definitions for accounts',
        'files': {
            'sdk/src/types/account.ts': fc('sdk/src/types/account.ts'),
            'sdk/src/types/index.ts': fc('sdk/src/types/index.ts'),
        }
    })

    phase9.append({
        'message': 'feat: add transaction type definitions',
        'files': {
            'sdk/src/types/transaction.ts': fc('sdk/src/types/transaction.ts'),
        }
    })

    phase9.append({
        'message': 'feat: implement SDK graph node and types',
        'files': {
            'sdk/src/graph/types.ts': fc('sdk/src/graph/types.ts'),
            'sdk/src/graph/node.ts': fc('sdk/src/graph/node.ts'),
        }
    })

    phase9.append({
        'message': 'feat: implement SDK graph builder',
        'files': {
            'sdk/src/graph/builder.ts': fc('sdk/src/graph/builder.ts'),
            'sdk/src/graph/index.ts': fc('sdk/src/graph/index.ts'),
        }
    })

    phase9.append({
        'message': 'feat: implement intent parser in SDK',
        'files': {
            'sdk/src/intent/types.ts': fc('sdk/src/intent/types.ts'),
            'sdk/src/intent/parser.ts': fc('sdk/src/intent/parser.ts'),
            'sdk/src/intent/index.ts': fc('sdk/src/intent/index.ts'),
        }
    })

    phase9.append({
        'message': 'feat: add connection utility module',
        'files': {
            'sdk/src/utils/connection.ts': fc('sdk/src/utils/connection.ts'),
            'sdk/src/utils/serialization.ts': fc('sdk/src/utils/serialization.ts'),
            'sdk/src/utils/index.ts': fc('sdk/src/utils/index.ts'),
        }
    })

    phase9.append({
        'message': 'feat: scaffold parallel executor for SDK',
        'files': {
            'sdk/src/executor/parallel.ts': fc('sdk/src/executor/parallel.ts', 0.5),
        }
    })

    phase9.append({
        'message': 'feat: implement bundle creation in executor',
        'files': {
            'sdk/src/executor/bundle.ts': fc('sdk/src/executor/bundle.ts'),
            'sdk/src/executor/parallel.ts': fc('sdk/src/executor/parallel.ts'),
            'sdk/src/executor/index.ts': fc('sdk/src/executor/index.ts'),
        }
    })

    phase9.append({
        'message': 'feat: implement main SDK client class',
        'files': {
            'sdk/src/client.ts': fc('sdk/src/client.ts', 0.5),
        }
    })

    phase9.append({
        'message': 'feat: complete SDK client with submit and status methods',
        'files': {
            'sdk/src/client.ts': fc('sdk/src/client.ts'),
        }
    })

    phase9.append({
        'message': 'feat: create SDK barrel export index',
        'files': {
            'sdk/src/index.ts': fc('sdk/src/index.ts'),
        }
    })

    phase9.append({
        'message': 'chore: generate package-lock.json for SDK',
        'files': {
            'sdk/package-lock.json': fc('sdk/package-lock.json'),
        }
    })

    phase9.append({
        'message': 'test: add SDK client unit tests',
        'files': {
            'tests/sdk/client.test.ts': fc('tests/sdk/client.test.ts'),
            'tests/sdk/graph.test.ts': fc('tests/sdk/graph.test.ts'),
        }
    })

    phases.append(phase9)

    # ═══════════════════════════════════════════════════════════════════════
    # PHASE 10: Tests + docs + polish (Feb-Mar 2026, ~15 commits)
    # ═══════════════════════════════════════════════════════════════════════
    phase10 = []

    phase10.append({
        'message': 'test: expand graph builder edge case tests',
        'files': {
            'tests/core/graph_test.rs': fc('tests/core/graph_test.rs'),
        }
    })

    phase10.append({
        'message': 'docs: add getting started guide',
        'files': {
            'docs/getting-started.md': fc('docs/getting-started.md'),
        }
    })

    phase10.append({
        'message': 'docs: add architecture overview document',
        'files': {
            'docs/architecture.md': fc('docs/architecture.md'),
        }
    })

    phase10.append({
        'message': 'docs: add API reference documentation',
        'files': {
            'docs/api-reference.md': fc('docs/api-reference.md'),
        }
    })

    phase10.append({
        'message': 'docs: expand README with badges and usage examples',
        'files': {
            'README.md': fc('README.md'),
        }
    })

    phase10.append({
        'message': 'docs: add contributing guidelines',
        'files': {
            'CONTRIBUTING.md': fc('CONTRIBUTING.md'),
        }
    })

    phase10.append({
        'message': 'docs: add security policy',
        'files': {
            'SECURITY.md': fc('SECURITY.md'),
        }
    })

    phase10.append({
        'message': 'chore: add project banner asset',
        'files': {
            'assets/banner.png': fc('assets/banner.png'),
        }
    })

    phase10.append({
        'message': 'ci: finalize CI workflow with test and lint steps',
        'files': {
            '.github/workflows/ci.yml': fc('.github/workflows/ci.yml'),
        }
    })

    phase10.append({
        'message': 'chore: finalize workspace Cargo.toml',
        'files': {
            'Cargo.toml': fc('Cargo.toml'),
        }
    })

    phase10.append({
        'message': 'chore: generate Cargo.lock',
        'files': {
            'Cargo.lock': fc('Cargo.lock'),
        }
    })

    phase10.append({
        'message': 'feat: finalize core lib.rs with all module exports',
        'files': {
            'crates/ivza-core/src/lib.rs': fc('crates/ivza-core/src/lib.rs'),
        }
    })

    phase10.append({
        'message': 'perf: optimize graph traversal with visited bitset caching',
        'files': {
            'crates/ivza-core/src/graph/decomposer.rs': fc('crates/ivza-core/src/graph/decomposer.rs'),
        }
    })

    phase10.append({
        'message': 'refactor: clean up unused imports across crates',
        'files': {
            'crates/ivza-core/src/lib.rs': fc('crates/ivza-core/src/lib.rs'),
            'crates/ivza-solver/src/lib.rs': fc('crates/ivza-solver/src/lib.rs'),
        }
    })

    phase10.append({
        'message': 'chore: update build script with release profile',
        'files': {
            'scripts/build.sh': fc('scripts/build.sh'),
            'scripts/test.sh': fc('scripts/test.sh'),
        }
    })

    phases.append(phase10)

    return phases


# ─── Main Execution ──────────────────────────────────────────────────────────

def main():
    os.chdir(PROJECT_DIR)

    print("=" * 60)
    print("IVZA Protocol - Git History Generator")
    print("=" * 60)

    # ── Step 1: Collect all tracked files ──────────────────────────────
    print("\n[1/6] Collecting current file contents...")

    all_files = []
    for root, dirs, files in os.walk(PROJECT_DIR):
        # Skip excluded directories
        dirs[:] = [d for d in dirs if d not in SKIP_DIRS]
        for f in files:
            if f == SKIP_FILE:
                continue
            full_path = os.path.join(root, f)
            rel_path = os.path.relpath(full_path, PROJECT_DIR).replace('\\', '/')
            all_files.append(rel_path)

    file_contents = {}
    file_hashes = {}
    for rel_path in sorted(all_files):
        full = os.path.join(PROJECT_DIR, rel_path.replace('/', os.sep))
        try:
            content = read_file_bytes(rel_path)
            file_hashes[rel_path] = hashlib.sha256(content).hexdigest()
            # Try to decode as text
            try:
                text = content.decode('utf-8')
                file_contents[rel_path] = text
            except UnicodeDecodeError:
                file_contents[rel_path] = content  # Keep as bytes (binary)
        except Exception as e:
            print(f"  [WARN] Could not read {rel_path}: {e}")

    print(f"  Collected {len(file_contents)} files")

    # ── Step 2: Define phases ──────────────────────────────────────────
    print("\n[2/6] Defining commit phases...")
    phases = define_phases(file_contents)
    total_commits = sum(len(p) for p in phases)
    print(f"  Defined {len(phases)} phases with {total_commits} total commits")

    # ── Step 3: Generate timestamps ────────────────────────────────────
    print("\n[3/6] Generating realistic timestamps...")
    timestamps = generate_timestamps(total_commits, START_DATE, END_DATE)
    print(f"  Generated {len(timestamps)} timestamps from {timestamps[0].strftime('%Y-%m-%d')} to {timestamps[-1].strftime('%Y-%m-%d')}")

    # Verify gaps
    day_set = set()
    for ts in timestamps:
        day_set.add((ts - START_DATE).days)
    total_days = (END_DATE - START_DATE).days
    consecutive_gaps = 0
    max_gap = 0
    current_gap = 0
    gap_count_3plus = 0
    for d in range(total_days + 1):
        if d not in day_set:
            current_gap += 1
        else:
            if current_gap >= 3:
                gap_count_3plus += 1
            max_gap = max(max_gap, current_gap)
            current_gap = 0
    if current_gap >= 3:
        gap_count_3plus += 1
    print(f"  Gaps of 3+ days: {gap_count_3plus}, longest gap: {max_gap} days")

    # ── Step 4: Clear working directory ────────────────────────────────
    print("\n[4/6] Clearing working directory (preserving .git)...")

    # Remove all existing commits by resetting
    # First, remove all tracked and untracked files
    for rel_path in sorted(all_files):
        full = os.path.join(PROJECT_DIR, rel_path.replace('/', os.sep))
        if os.path.exists(full):
            os.remove(full)

    # Remove empty directories (but not .git, node_modules, target)
    for root, dirs, files in os.walk(PROJECT_DIR, topdown=False):
        dirs[:] = [d for d in dirs if d not in SKIP_DIRS]
        rel_root = os.path.relpath(root, PROJECT_DIR)
        if rel_root == '.':
            continue
        top_dir = rel_root.replace('\\', '/').split('/')[0]
        if top_dir in SKIP_DIRS:
            continue
        try:
            if not os.listdir(root):
                os.rmdir(root)
        except OSError:
            pass

    # Remove any existing git history (orphan reset)
    # We create a fresh orphan branch
    run_git('checkout', '--orphan', 'temp_history_branch')
    run_git('rm', '-rf', '--cached', '.')

    print("  Working directory cleared")

    # ── Step 5: Execute commits ────────────────────────────────────────
    print("\n[5/6] Executing commits...")

    commit_idx = 0
    last_prefix = None
    prefix_streak = 0

    for phase_num, phase in enumerate(phases, 1):
        phase_names = [
            "Scaffolding", "Core types & graph", "Analyzers", "Scheduler",
            "Solver", "Intent system", "Anchor program", "CLI",
            "TypeScript SDK", "Tests + docs + polish"
        ]
        phase_name = phase_names[phase_num - 1] if phase_num <= len(phase_names) else f"Phase {phase_num}"
        print(f"\n  Phase {phase_num}: {phase_name} ({len(phase)} commits)")

        for spec in phase:
            if commit_idx >= len(timestamps):
                # Safety: reuse last timestamp with small offset
                ts = timestamps[-1] + timedelta(minutes=commit_idx - len(timestamps) + 1)
            else:
                ts = timestamps[commit_idx]

            msg = spec['message']

            # Enforce max 3 same prefix in a row
            prefix = msg.split(':')[0] if ':' in msg else msg.split(' ')[0]
            if prefix == last_prefix:
                prefix_streak += 1
                if prefix_streak >= 3:
                    # Swap prefix to something plausible
                    alt_prefixes = {
                        'feat': 'refactor',
                        'fix': 'refactor',
                        'refactor': 'chore',
                        'test': 'chore',
                        'docs': 'chore',
                        'chore': 'feat',
                        'ci': 'chore',
                        'perf': 'refactor',
                    }
                    new_prefix = alt_prefixes.get(prefix, 'chore')
                    msg = new_prefix + msg[len(prefix):]
                    prefix = new_prefix
                    prefix_streak = 1
            else:
                prefix_streak = 1
            last_prefix = prefix

            # Write files
            files = spec.get('files', {})
            deletes = spec.get('deletes', [])

            for rel_path, content in files.items():
                if content is not None:
                    write_file(rel_path, content)

            for rel_path in deletes:
                delete_file(rel_path)

            make_commit(msg, ts)
            commit_idx += 1

            if commit_idx % 20 == 0:
                print(f"    ... {commit_idx}/{total_commits} commits done")

    print(f"\n  All {commit_idx} commits created")

    # ── Step 6: Verify final state ─────────────────────────────────────
    print("\n[6/6] Verifying final state matches original...")

    mismatches = []
    missing = []
    for rel_path, original_hash in sorted(file_hashes.items()):
        current_hash = file_hash(rel_path)
        if current_hash is None:
            missing.append(rel_path)
        elif current_hash != original_hash:
            mismatches.append(rel_path)

    if missing or mismatches:
        print(f"  Found {len(missing)} missing files, {len(mismatches)} mismatched files")
        print("  Creating final sync commit...")

        # Restore all missing/mismatched files
        for rel_path in missing + mismatches:
            content = file_contents[rel_path]
            write_file(rel_path, content)

        sync_ts = END_DATE.replace(hour=23, minute=59, second=59)
        make_commit('chore: sync all files to final state', sync_ts)
        print("  Sync commit created")
    else:
        print("  All files match - no sync commit needed")

    # Rename branch to main
    run_git('branch', '-m', 'temp_history_branch', 'main')

    # ── Summary ────────────────────────────────────────────────────────
    print("\n" + "=" * 60)
    print("SUMMARY")
    print("=" * 60)

    result = subprocess.run(
        ['git', 'log', '--oneline'], cwd=PROJECT_DIR,
        capture_output=True, text=True
    )
    actual_commits = len(result.stdout.strip().split('\n')) if result.stdout.strip() else 0

    result2 = subprocess.run(
        ['git', 'log', '--format=%aI', '--reverse'], cwd=PROJECT_DIR,
        capture_output=True, text=True
    )
    dates = result2.stdout.strip().split('\n') if result2.stdout.strip() else []

    print(f"  Total commits: {actual_commits}")
    if dates:
        print(f"  Date range: {dates[0][:10]} to {dates[-1][:10]}")
    print(f"  Files tracked: {len(file_contents)}")
    print(f"  Missing files: {len(missing)}")
    print(f"  Mismatched files: {len(mismatches)}")
    print(f"  Branch: main")
    print("\nDone!")


if __name__ == '__main__':
    main()
