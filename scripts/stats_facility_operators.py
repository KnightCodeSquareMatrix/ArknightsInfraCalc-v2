import json

operbox = json.load(open('data/fixtures/243/operbox_full_e2.json', encoding='utf-8'))
owned_names = {op['name'] for op in operbox if op.get('own')}

table = json.load(open('data/skill_table.json', encoding='utf-8'))
skills = table['skills'] if 'skills' in table else []

instances_data = json.load(open('data/operator_instances.json', encoding='utf-8'))
instances = instances_data['instances']
trade_names = set(); manu_names = set(); power_names = set(); control_names = set()
for entry in instances.values():
    facilities = entry.get('facilities', {})
    name = entry['name']
    if 'trade' in facilities: trade_names.add(name)
    if 'manufacture' in facilities: manu_names.add(name)
    if 'power' in facilities: power_names.add(name)
    if 'control' in facilities: control_names.add(name)

def count_owned(s): return len([n for n in s if n in owned_names])

rotating = trade_names | manu_names | power_names

print(f'===== 全精2练度盒 (data/fixtures/243/operbox_full_e2.json) =====')
print(f'拥有干员: {len(owned_names)}')
print(f'')
print(f'  设施技能持有:')
print(f'    贸易站: {count_owned(trade_names)}')
print(f'    制造站: {count_owned(manu_names)}')
print(f'    发电站: {count_owned(power_names)}')
print(f'    控制中枢: {count_owned(control_names)}')
print(f'    轮转岗(贸+制+电): {len([n for n in rotating if n in owned_names])}')
print(f'')

# 公孙盒
gs = json.load(open('data/operbox_gongsun.json', encoding='utf-8-sig'))
gs_owned = {op['name'] for op in gs if op.get('own')}
gs_trade = len([n for n in trade_names if n in gs_owned])
gs_manu = len([n for n in manu_names if n in gs_owned])
gs_power = len([n for n in power_names if n in gs_owned])
gs_ctrl = len([n for n in control_names if n in gs_owned])
gs_rotating = len([n for n in rotating if n in gs_owned])

print(f'===== 公孙盒 (data/operbox_gongsun.json) =====')
print(f'拥有干员: {len(gs_owned)}')
print(f'')
print(f'  设施技能持有:')
print(f'    贸易站: {gs_trade}')
print(f'    制造站: {gs_manu}')
print(f'    发电站: {gs_power}')
print(f'    控制中枢: {gs_ctrl}')
print(f'    轮转岗(贸+制+电): {gs_rotating}')
print(f'')

print(f'===== 243 蓝图岗位需求 =====')
print(f'  贸易站: 3站 × 3人 = 9')
print(f'  制造站: 4站 × 3人 = 12')
print(f'  发电站: 3站 × 1人 = 3')
print(f'  轮转岗合计: 24人/班')
print(f'')
print(f'===== αβγ 三队均衡轮休 =====')
print(f'  轮换模型: 每班 2/3 上岗, 1/3 休息')
print(f'  公式: 轮转池 = 24 × 3/2 = 36人')
print(f'')
print(f'  全精2盒: {len([n for n in rotating if n in owned_names])} >= 36? {"✅" if len([n for n in rotating if n in owned_names]) >= 36 else "❌"}')
print(f'  公孙盒:  {gs_rotating} >= 36? {"✅" if gs_rotating >= 36 else "❌"}')
print(f'')
print(f'  --- 如果按效率分层 ---')
print(f'  长班1 (顶峰): meta 全上, 24人')
print(f'  长班2 (次优): 次优干员, 24人')
print(f'  短班 (低效):  低效干员, 24人')
print(f'  这种情况需要: 24+12+12 = 48人轮转池')
print(f'  全精2盒48+? {"✅" if len([n for n in rotating if n in owned_names]) >= 48 else "❌ 不够"}')
print(f'  公孙盒48+?  {"✅" if gs_rotating >= 48 else "❌ 不够"}')
