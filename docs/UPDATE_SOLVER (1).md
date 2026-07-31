# 更新服务器求解器步骤

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/FRONTEND_SERVE_GUIDE.md
> 摘要：记录服务器手工替换 infra-cli、重启 Next 和回滚的操作步骤

在本地或构建机准备 Linux 版 `infra-cli`。不要上传 Windows 版 `infra-cli.exe`。

建议先记录新文件哈希，方便确认服务器上实际替换的是同一个文件：

```bash
sha256sum ./infra-cli
```

上传到服务器临时路径：

```bash
scp ./infra-cli root@110.42.36.46:/root/infra-cli.new
```

## 标准更新流程

登录服务器：

```bash
ssh root@110.42.36.46
```

执行更新：

```bash
set -euo pipefail

app=/root/ArknightsInfraCalc-v2_beta_test_frontend-main
new_cli=/root/infra-cli.new
ts=$(date +%Y%m%d%H%M%S)

cd "$app"

test -f "$new_cli"
test -f bin/infra-cli

echo "Current solver:"
sha256sum bin/infra-cli || true

echo "New solver:"
sha256sum "$new_cli" || true

cp -a bin/infra-cli "bin/infra-cli.backup-$ts"
cp "$new_cli" "bin/infra-cli.new-$ts"
chmod +x "bin/infra-cli.new-$ts"
mv -f "bin/infra-cli.new-$ts" bin/infra-cli
chmod +x bin/infra-cli

echo "Installed solver:"
sha256sum bin/infra-cli || true
```

此时文件已经替换，但线上正在运行的 `infra-cli serve` 仍可能是旧进程。继续重启 Next 进程：

```bash
cd /root/ArknightsInfraCalc-v2_beta_test_frontend-main

old_pid=$(ss -ltnp 'sport = :4174' | sed -n 's/.*pid=\([0-9]\+\).*/\1/p' | head -1)
if [ -n "${old_pid:-}" ]; then
  children=$(pgrep -P "$old_pid" || true)
  [ -n "$children" ] && kill $children || true
  kill "$old_pid" || true
  sleep 2
  kill -0 "$old_pid" 2>/dev/null && kill -9 "$old_pid" || true
fi

nohup ./node_modules/.bin/next start -H 0.0.0.0 -p 4174 > server/next.log 2>&1 &
```

## 发布后验证

确认端口已经重新监听：

```bash
ss -ltnp | grep ':4174'
```

检查健康接口：

```bash
curl -sS http://127.0.0.1:4174/api/health
```

期望结果里至少满足：

- `ok: true`
- `cliReady: true`
- `serveError` 为空或不存在

查看启动日志：

```bash
tail -n 80 /root/ArknightsInfraCalc-v2_beta_test_frontend-main/server/next.log
```

## 回滚

如果健康检查失败、页面崩溃或新求解器不能启动，优先回滚到更新前自动备份的 `bin/infra-cli.backup-<timestamp>`。

```bash
set -euo pipefail

app=/root/ArknightsInfraCalc-v2_beta_test_frontend-main
backup=/root/ArknightsInfraCalc-v2_beta_test_frontend-main/bin/infra-cli.backup-<timestamp>

cd "$app"
test -f "$backup"

cp -a bin/infra-cli "bin/infra-cli.failed-$(date +%Y%m%d%H%M%S)"
cp "$backup" bin/infra-cli
chmod +x bin/infra-cli

old_pid=$(ss -ltnp 'sport = :4174' | sed -n 's/.*pid=\([0-9]\+\).*/\1/p' | head -1)
if [ -n "${old_pid:-}" ]; then
  children=$(pgrep -P "$old_pid" || true)
  [ -n "$children" ] && kill $children || true
  kill "$old_pid" || true
  sleep 2
  kill -0 "$old_pid" 2>/dev/null && kill -9 "$old_pid" || true
fi

nohup ./node_modules/.bin/next start -H 0.0.0.0 -p 4174 > server/next.log 2>&1 &
curl -sS http://127.0.0.1:4174/api/health
```
