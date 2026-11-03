#!/usr/bin/env python3
"""List upstream operators whose building buff IDs are absent locally."""
import argparse
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--building', required=True, type=Path,
                        help='Upstream building_data.json or building_skills.json')
    parser.add_argument('--characters', required=True, type=Path,
                        help='Upstream character_table.json from the same commit')
    parser.add_argument('--skill-table', type=Path, default=ROOT / 'data/skill_table.json')
    parser.add_argument('--instances', type=Path, default=ROOT / 'data/operator_instances.json')
    parser.add_argument('--facility', action='append', help='Upstream roomType, e.g. MANUFACTURE; repeatable')
    parser.add_argument('--json', action='store_true', help='Print full machine-readable report')
    args = parser.parse_args()
    building, characters = read(args.building), read(args.characters)
    skills, instances = read(args.skill_table)['skills'], read(args.instances)['instances']
    if not isinstance(skills, list) or not isinstance(instances, dict):
        raise ValueError('Expected skills array and instances object')
    if not isinstance(characters, dict) or any(not isinstance(building.get(k), dict) for k in ('buffs', 'chars')):
        raise ValueError('Expected upstream character object and building buffs/chars objects')
    local_ids = {skill['id'] for skill in skills}
    if len(local_ids) != len(skills):
        raise ValueError('Duplicate local skill IDs')
    local_names = {instance['name'] for instance in instances.values()}
    rooms = {room.upper() for room in args.facility or []}
    available_rooms = {buff['roomType'] for buff in building['buffs'].values()}
    if rooms - available_rooms:
        raise ValueError(f'Unknown facilities: {sorted(rooms - available_rooms)}')
    rows, absent_instances = [], []
    operators_with_skills = 0
    for char_id, char in sorted(building['chars'].items()):
        entries = {}
        for group in char['buffChar']:
            for binding in group['buffData']:
                buff_id = binding['buffId']
                buff = building['buffs'][buff_id]
                if rooms and buff['roomType'] not in rooms:
                    continue
                entry = entries.setdefault(buff_id, {
                    'buff_id': buff_id, 'skill_name': buff['buffName'],
                    'facility': buff['roomType'], 'description': buff['description'], 'unlocks': [],
                })
                if binding['cond'] not in entry['unlocks']:
                    entry['unlocks'].append(binding['cond'])
        if not entries:
            continue
        operators_with_skills += 1
        name = characters[char_id]['name']
        absent = name not in local_names
        if absent:
            absent_instances.append({'char_id': char_id, 'name': name})
        missing = [entry for buff_id, entry in entries.items() if buff_id not in local_ids]
        if missing:
            rows.append({'char_id': char_id, 'name': name, 'instance_absent': absent,
                         'upstream_skill_count': len(entries), 'missing_skills': missing})
    report = {
        'comparison': 'Exact upstream buff ID membership; gaps do not prove missing mechanics or incorrect unlocks.',
        'inputs': {key: str(getattr(args, key).resolve()) for key in ('building', 'characters', 'skill_table', 'instances')},
        'facilities': sorted(rooms), 'local_skill_count': len(local_ids),
        'upstream_operators_with_skills': operators_with_skills,
        'operators_with_missing_skills': len(rows),
        'unique_missing_buff_count': len({skill['buff_id'] for row in rows for skill in row['missing_skills']}),
        'missing_instances': absent_instances, 'operators': rows,
    }
    if args.json:
        print(json.dumps(report, ensure_ascii=False, indent=2))
    else:
        print(f"本地技能 {len(local_ids)}；上游有基建技能角色 {operators_with_skills}；技能 ID 未收录角色 {len(rows)}；角色实例缺失 {len(absent_instances)}")
        print('仅比较原始 ID；自定义 ID、委托及未支持设施需另行判断。')
        print('角色名	角色ID	实例缺失	未收录/上游技能数	未收录技能（设施:名称:ID）')
        for row in rows:
            details = '; '.join(f"{s['facility']}:{s['skill_name']}:{s['buff_id']}" for s in row['missing_skills'])
            print(chr(9).join([row['name'], row['char_id'], '是' if row['instance_absent'] else '否',
                              f"{len(row['missing_skills'])}/{row['upstream_skill_count']}", details]))
        covered_absent = [item for item in absent_instances if item['char_id'] not in {row['char_id'] for row in rows}]
        if covered_absent:
            print('技能 ID 均已收录、但角色实例缺失：' + '、'.join(f"{x['name']}({x['char_id']})" for x in covered_absent))


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, KeyError, TypeError, AttributeError) as error:
        sys.exit(f'Comparison failed: {error}')
