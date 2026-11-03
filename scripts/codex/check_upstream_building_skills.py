#!/usr/bin/env python3
"""Download building tables from GitHub raw and report local coverage candidates."""
import argparse
import hashlib
import json
import re
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import quote

from report_missing_building_skills import ROOT, read

SOURCES = {
    'yuanyan3060/ArknightsGameResource': ('main', 'gamedata/excel'),
    'Kengxxiao/ArknightsGameData': ('master', 'zh_CN/gamedata/excel'),
}


def write_json(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')


def fetch(url, path, proxy, timeout):
    result = subprocess.run(
        ['curl', '--http1.1', '--fail', '--location', '--silent', '--show-error',
         '--compressed', '--connect-timeout', '10', '--max-time', str(timeout),
         '--proxy', proxy, '-H', 'User-Agent: arknights-building-skills',
         '--output', str(path), url], capture_output=True, timeout=timeout + 5,
    )
    if result.returncode:
        raise RuntimeError(f'{url}: {result.stderr.decode(errors="replace").strip()}')
    read(path)


def render(report, source):
    version_note = (f"两份文件请求同一提交格式 ref：{source['requested_commit']}，未独立解析提交身份。"
                    if source['requested_commit'] else
                    '分支 ref 未解析为提交；不证明两份文件来自同一提交，固定版本请传完整提交 SHA。')
    lines = ['# 上游基建技能差集', '',
             f"来源：{source['repo']}，请求 ref：{source['ref']}。",
             '直接下载 GitHub raw；' + version_note, '',
             f"名册校验：{report['coverage']['playable_operators']} 名可获得干员与基建名册一致。",
             f"全部缺实例：{len(report['all_missing_instances'])}；范围内缺实例：{len(report['missing_instances'])}；"
             f"原始技能 ID 差集：{report['unique_missing_buff_count']}。", '',
             '精确 ID 差集是待核对候选，不证明机制缺失；已有 ID 的角色绑定、别名、委托及实际结算仍需覆盖审计。', '',
             '## 全部缺实例干员（不受技能筛选影响）', '',
             '、'.join(item['name'] for item in report['all_missing_instances']) or '无', '']

    def add_operator(row, key):
        lines.extend([f"### {row['name']}（{row['char_id']}）", ''])
        for skill in row[key]:
            unlocks = ' / '.join(f"E{u['phase']} Lv.{u['level']}" for u in skill['unlocks'])
            description = re.sub(r'<[^>]*>', '', skill['description'])
            lines.extend([f"- {skill['facility']} · {skill['skill_name']} · {unlocks} · {skill['buff_id']}",
                          f"  {description}"])
        lines.append('')

    lines.extend(['## 缺实例干员的范围内技能', ''])
    for row in report['missing_operator_skills_in_scope']:
        add_operator(row, 'skills')
    if not report['missing_operator_skills_in_scope']:
        lines.extend(['无', ''])
    lines.extend(['## 已有干员的原始技能 ID 差集', ''])
    existing = [row for row in report['operators'] if not row['instance_absent']]
    for row in existing:
        add_operator(row, 'missing_skills')
    if not existing:
        lines.extend(['无', ''])
    return '\n'.join(lines) + '\n'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', choices=SOURCES, default=next(iter(SOURCES)))
    parser.add_argument('--ref', help='Branch, tag or full commit SHA; defaults to source branch')
    parser.add_argument('--proxy', default='http://127.0.0.1:7890', help='Pass empty string for direct access')
    parser.add_argument('--timeout', type=int, default=60, help='Maximum seconds per download')
    parser.add_argument('--output-dir', type=Path, help='New directory under out/ or target/codex-runs/')
    parser.add_argument('--data-dir', type=Path, default=ROOT / 'data')
    parser.add_argument('--facility', action='append', help='Upstream roomType; repeatable')
    parser.add_argument('--all-skills', action='store_true', help='Disable the default valuable filter')
    args = parser.parse_args()
    if args.timeout < 1 or args.timeout > 300:
        parser.error('--timeout must be between 1 and 300 seconds')
    now = datetime.now(timezone.utc)
    output = (args.output_dir or ROOT / 'out/building-skills' / now.strftime('%Y%m%dT%H%M%S%fZ')).resolve()
    roots = [(ROOT / name).resolve() for name in ('out', 'target/codex-runs')]
    if not any(output != root and output.is_relative_to(root) for root in roots):
        parser.error('--output-dir must be a new subdirectory of out/ or target/codex-runs/')
    output.mkdir(parents=True, exist_ok=False)
    branch, directory = SOURCES[args.repo]
    ref = args.ref or branch
    pinned = bool(re.fullmatch(r'[0-9a-fA-F]{40}', ref))
    source = {'repo': args.repo, 'ref': ref, 'retrieved_at_utc': now.isoformat(),
              'transport': 'github_raw', 'commit': None, 'requested_commit': ref if pinned else None,
              'version_guarantee': 'commit_ref_unverified' if pinned else 'unresolved_ref',
              'files': {}, 'status': 'downloading'}
    write_json(output / 'source.json', source)
    try:
        for filename in ('building_data.json', 'character_table.json'):
            url = f'https://raw.githubusercontent.com/{args.repo}/{quote(ref, safe="")}/{directory}/{filename}'
            print(f'Downloading {filename}', file=sys.stderr, flush=True)
            fetch(url, output / filename, args.proxy, args.timeout)
            source['files'][filename] = {'url': url, 'sha256': hashlib.sha256((output / filename).read_bytes()).hexdigest()}
            write_json(output / 'source.json', source)
        command = [sys.executable, str(Path(__file__).with_name('report_missing_building_skills.py')),
                   '--building', str(output / 'building_data.json'),
                   '--characters', str(output / 'character_table.json'),
                   '--skill-table', str(args.data_dir / 'skill_table.json'),
                   '--instances', str(args.data_dir / 'operator_instances.json'), '--json']
        if not args.all_skills:
            command.append('--valuable')
        for facility in args.facility or []:
            command.extend(['--facility', facility])
        result = subprocess.run(command, capture_output=True, text=True)
        if result.returncode:
            raise RuntimeError(result.stderr.strip() or f'Comparison exited {result.returncode}')
        report = json.loads(result.stdout)
        building = read(output / 'building_data.json')
        write_json(output / 'building_skills.json', {key: building[key] for key in ('buffs', 'chars')})
        write_json(output / 'comparison.json', report)
        (output / '缺失干员与技能.md').write_text(render(report, source), encoding='utf-8')
        source.update(status='completed', buff_count=len(building['buffs']), char_count=len(building['chars']))
        write_json(output / 'source.json', source)
        print(json.dumps({'output_dir': str(output), 'coverage': report['coverage'],
                          'valuable_filter': report['valuable_filter'],
                          'missing_instances_in_scope': len(report['missing_instances']),
                          'unique_missing_buff_ids': report['unique_missing_buff_count'],
                          'existing_operators_with_missing_ids': sum(not r['instance_absent'] for r in report['operators']),
                          'version_guarantee': source['version_guarantee'],
                          'report': str(output / '缺失干员与技能.md')}, ensure_ascii=False, indent=2))
    except Exception as error:
        source.update(status='failed', error=str(error))
        write_json(output / 'source.json', source)
        raise


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, KeyError, TypeError, AttributeError, RuntimeError, subprocess.TimeoutExpired) as error:
        sys.exit(f'Upstream check failed: {error}')
