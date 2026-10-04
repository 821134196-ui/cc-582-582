#!/usr/bin/env python3
"""端到端验证三大核心不变量：设备互斥 / 回路功率阈值 / 取消后资源立即恢复，
   以及：换灯延时重新确认、自审批豁免拦截、归还与完工流程、权限边界。"""
import json, urllib.request, urllib.error, sys, time, datetime

BASE = "http://localhost:" + __import__("os").environ.get("PORT", "8080") + "/api"
PASS, FAIL = [], []

def call(method, path, uid, body=None, expect_status=None):
    req = urllib.request.Request(BASE + path, method=method)
    req.add_header("Content-Type", "application/json")
    if uid: req.add_header("X-User-Id", str(uid))
    data = json.dumps(body).encode() if body is not None else None
    try:
        with urllib.request.urlopen(req, data=data) as r:
            return r.status, json.loads(r.read() or b"null")
    except urllib.error.HTTPError as e:
        raw = e.read().decode()
        try: payload = json.loads(raw)
        except Exception: payload = {"error": raw}
        return e.code, payload

def check(name, cond, detail=""):
    (PASS if cond else FAIL).append(name)
    print(("  ✅" if cond else "  ❌"), name, ("— " + str(detail) if detail and not cond else "") or ("" if cond else ""))

def iso(day, h1, m1=0, h2=None, m2=0):
    d1 = datetime.datetime(2026, 10, day, h1, m1, tzinfo=datetime.timezone.utc)
    d2 = datetime.datetime(2026, 10, day, h2 if h2 else h1+8, m2, tzinfo=datetime.timezone.utc)
    return d1.isoformat().replace("+00:00","Z"), d2.isoformat().replace("+00:00","Z")

# 灯具: 1=1800W,2=1800W(回路3 上限6000), 3=450W,4=450W(回路1 上限2400),
#       5=720W,6=720W(回路2 上限3000), 7=2000W 维修中, 8=2400W
s, e = iso(20, 9, 0, 18)

print("\n=== 0. 基础数据 ===")
st, studios = call("GET", "/studios", 3)
A = studios[0]
check("场地A有3个回路8盏灯", len(A["circuits"])==3 and len(A["fixtures"])==8)

print("\n=== 1. 设备互斥：同一灯具不能被两组同时占用 ===")
st, p1 = call("POST", "/projects", 1, {"name":"项目甲-时尚大片","studio_id":1,"start_at":s,"end_at":e,
    "items":[{"fixture_id":1,"circuit_id":3,"x":2.0,"y":2.0},{"fixture_id":3,"circuit_id":1,"x":3.0,"y":2.0}]})
pid1 = p1["id"]
call("POST", f"/projects/{pid1}/submit", 1)
st, _ = call("POST", f"/projects/{pid1}/confirm", 3, {"decision":"confirmed"})
check("场务确认甲成功 → 200", st==200, st)

# 乙在重叠时间占用同一盏灯1
st, p2 = call("POST", "/projects", 2, {"name":"项目乙-冲突测试","studio_id":1,"start_at":s,"end_at":e,
    "items":[{"fixture_id":1,"circuit_id":3,"x":4.0,"y":4.0}]})
pid2 = p2["id"]
call("POST", f"/projects/{pid2}/submit", 2)
st, body = call("POST", f"/projects/{pid2}/confirm", 3, {"decision":"confirmed"})
check("乙占用同灯1被拒 → 409", st==409, st)
check("错误信息说明灯具互斥", "灯具" in body.get("error","") or "report" in body.get("error",""), str(body)[:200])

# 不重叠时段（次日）应成功
s2, e2 = iso(21, 9, 0, 18)
st, _ = call("PUT", f"/projects/{pid2}", 2, {"studio_id":1,"start_at":s2,"end_at":e2,
    "items":[{"fixture_id":1,"circuit_id":3,"x":4.0,"y":4.0}]})
st, _ = call("POST", f"/projects/{pid2}/submit", 2)
st, _ = call("POST", f"/projects/{pid2}/confirm", 3, {"decision":"confirmed"})
check("乙改到次日（不重叠）可确认", st==200, st)

# 场地互斥：丙同棚同日但完全不同的灯，也应因场地冲突失败
st, p3 = call("POST", "/projects", 2, {"name":"项目丙-场地冲突","studio_id":1,"start_at":s,"end_at":e,
    "items":[{"fixture_id":5,"circuit_id":2,"x":5.0,"y":5.0}]})
pid3 = p3["id"]
call("POST", f"/projects/{pid3}/submit", 2)
st, body = call("POST", f"/projects/{pid3}/confirm", 3, {"decision":"confirmed"})
check("丙灯不同但场地时段重叠 → 409", st==409, str(body)[:150])

print("\n=== 2. 回路功率阈值：超限不得手工放行 ===")
# 回路2 上限3000W；灯5(720)+灯6(720)+灯8(2400) = 3840W 超限
st, p4 = call("POST", "/projects", 2, {"name":"项目丁-功率超限","studio_id":1,
    "start_at":s2,"end_at":e2,  # 次日，灯都空闲
    "items":[{"fixture_id":5,"circuit_id":2,"x":1.0,"y":1.0},
             {"fixture_id":6,"circuit_id":2,"x":2.0,"y":1.0},
             {"fixture_id":8,"circuit_id":2,"x":3.0,"y":1.0}]})
pid4 = p4["id"]
st, report = call("POST", "/validate", 2, {"studio_id":1,"start_at":s2,"end_at":e2,
    "items":[{"fixture_id":5,"circuit_id":2},{"fixture_id":6,"circuit_id":2},{"fixture_id":8,"circuit_id":2}]})
check("预检报告 ok=false", report["ok"] is False)
check("预检报告标注回路2超限", any(c["circuit_id"]==2 and c["overloaded"] for c in report["circuit_loads"]))
call("POST", f"/projects/{pid4}/submit", 2)
st, body = call("POST", f"/projects/{pid4}/confirm", 3, {"decision":"confirmed"})
check("场务确认超限方案被拒 → 409（无放行参数可用）", st==409, st)

# 叠加场景：乙次日在回路3占用灯1(1800)；丁' 次日再放灯1+2(各1800)到回路3 = 既有1800+3600=5400<6000 OK
st, report2 = call("POST", "/validate", 2, {"studio_id":1,"start_at":s2,"end_at":e2,
    "items":[{"fixture_id":1,"circuit_id":3},{"fixture_id":2,"circuit_id":3}], "exclude_project_id": None})
# 灯1次日已被乙占用——不排除乙时应报冲突
check("不排除乙时灯1冲突", not report2["ok"] and any(c["kind"]=="fixture" for c in report2["fixture_conflicts"]))

# 再叠加到刚好超限：回路3 已有乙1800；灯2(1800)+灯7不能用(维修)；用灯1被互斥。换思路：灯2+灯8(2400,默认回路2但可指定回路3) → 1800+1800+2400=6000 正好不超
st, r6000 = call("POST", "/validate", 2, {"studio_id":1,"start_at":s2,"end_at":e2,
    "items":[{"fixture_id":2,"circuit_id":3},{"fixture_id":8,"circuit_id":3}]})
c3 = [c for c in r6000["circuit_loads"] if c["circuit_id"]==3][0]
# 注意：此时乙持有 A 棚 10/21 场地，所以整体 ok=false（场地冲突）；但回路3的功率汇总是关键
check("回路3累计 6000W == 上限 6000W 不判超限（边界等于合法）", c3["total_after_w"]==6000 and not c3["overloaded"], f"{c3}")

print("\n=== 3. 取消项目后资源立即恢复可用 ===")
st, _ = call("POST", f"/projects/{pid1}/cancel", 1)
check("甲取消成功", st==200, st)
# 丙此前因场地冲突不能确认；甲取消后丙应立即可以确认
st, _ = call("POST", f"/projects/{pid3}/confirm", 3, {"decision":"confirmed"})
check("甲取消后丙立即确认成功（场地+灯均释放）", st==200, st)
# 时间线里甲的 booking 应全部 cancelled
st, tl = call("GET", "/timeline?studio_id=1", 3)
jia = [b for b in tl["bookings"] if b["project_id"]==pid1]
check("甲的灯具占用全部为 cancelled", jia and all(b["status"]=="cancelled" for b in jia), str([b["status"] for b in jia]))
jia_slots = [x for x in tl["slots"] if x["project_id"]==pid1]
check("甲的场地占用为 released 或不再 held", all(x["status"]!="held" for x in jia_slots))
# 乙占用次日灯1；取消乙后，丁改次日且把三灯都放回路2仍超限，但只放5+8=3120? 5(720)+8(2400)=3120>3000 仍超; 5+6=1440 OK
st, _ = call("POST", f"/projects/{pid2}/cancel", 3)
st, p5 = call("POST", "/projects", 1, {"name":"项目戊-取消后复用","studio_id":1,"start_at":s2,"end_at":e2,
    "items":[{"fixture_id":1,"circuit_id":3,"x":1.0,"y":1.0}]})
pid5 = p5["id"]
call("POST", f"/projects/{pid5}/submit", 1)
st, _ = call("POST", f"/projects/{pid5}/confirm", 3, {"decision":"confirmed"})
check("乙取消后灯1次日立即能被戊占用", st==200, st)

print("\n=== 4. 换灯/延时 → 重新计算占用并要求重新确认 ===")
# 戊已 confirmed。把时间延到与丙(20日)无关的21日更大窗口其实没冲突；延到21日 6-23 点仍独占灯1，没问题，但换灯换成灯5(720)+灯6(720) 同回路2=1440 OK
st, upd = call("PUT", f"/projects/{pid5}", 1, {"studio_id":1,"start_at":s2,"end_at":e2,
    "items":[{"fixture_id":5,"circuit_id":2,"x":1.0,"y":1.0},{"fixture_id":6,"circuit_id":2,"x":2.0,"y":1.0}]})
check("修改已确认方案成功", st==200, st)
check("返回 needs_reconfirm=true", upd.get("needs_reconfirm") is True)
st, p5d = call("GET", f"/projects/{pid5}", 1)
check("项目状态回到 pending", p5d["status"]=="pending")
check("旧占用已释放（无 held）", len(p5d["bookings"])==0)
check("保留两个版本", len(p5d["versions"])==2)
st, _ = call("POST", f"/projects/{pid5}/confirm", 3, {"decision":"confirmed"})
check("场务重新确认成功（新版本占用）", st==200, st)

print("\n=== 5. 逐件归还 / 损坏关联项目 / 自审批豁免拦截 ===")
st, _ = call("POST", f"/projects/{pid5}/start", 3)
st, p5d = call("GET", f"/projects/{pid5}", 3)
b_fixtures = [b["fixture_id"] for b in p5d["bookings"]]
# 第一件完好归还
st, r1 = call("POST", f"/projects/{pid5}/returns", 3, {"fixture_id":b_fixtures[0],"condition":"ok"})
check("归还第一件成功", st==200)
# 第二件损坏
st, r2 = call("POST", f"/projects/{pid5}/returns", 3, {"fixture_id":b_fixtures[1],"condition":"damaged","note":"灯架弯折，柔光箱破损"})
check("归还第二件（损坏）成功并产生 damage_id", st==200 and r2.get("damage_id"), st)
did = r2["damage_id"]
st, dmg = call("GET", f"/projects/{pid5}/damages", 3)
check("损坏记录关联本项目", dmg and dmg[0]["project_id"]==pid5 and dmg[0]["fixture_id"]==b_fixtures[1])
# 摄影师申请豁免
st, _ = call("POST", f"/damages/{did}/waiver/request", 1)
check("摄影师可申请豁免", st==200, st)
# 摄影师自己审批 → 必须拒绝
st, body = call("POST", f"/damages/{did}/waiver/decision", 1, {"approve":True,"note":"自己批"})
check("摄影师审批自己项目豁免 → 403", st==403, f"{st} {body}")
# 场务(3) 不是该项目摄影师 → 可批准
st, _ = call("POST", f"/damages/{did}/waiver/decision", 3, {"approve":True,"note":"非人为，正常损耗"})
check("其他场务审批豁免成功", st==200, st)
# 未归还完不能完工
st, body = call("POST", f"/projects/{pid5}/complete", 3)
# 两件都还了——应成功
check("两件都归还后可完工", st==200, f"{st} {body}")
st, tl = call("GET", "/timeline?studio_id=1", 3)
check("完工后场地释放", all(x["status"]!="held" for x in tl["slots"] if x["project_id"]==pid5))

print("\n=== 6. 权限边界与维修灯 ===")
st, _ = call("POST", f"/projects/{pid3}/confirm", 1, {"decision":"confirmed"})
check("摄影师不能确认任意项目 → 403", st==403, st)
st, _ = call("POST", f"/projects/{pid3}/start", 1)
check("摄影师不能开拍 → 403", st==403, st)
st, body = call("POST", "/projects", 3, {"name":"场务建方案","studio_id":1,"start_at":s,"end_at":e,
    "items":[{"fixture_id":5,"circuit_id":2}]})
check("场务不能提交拍摄方案 → 403", st==403, st)
# 维修灯7不能占用
s30,e30 = iso(30,9,0,18)
st, body = call("POST", "/validate", 1, {"studio_id":1,"start_at":s30,"end_at":e30,
    "items":[{"fixture_id":7,"circuit_id":3}]})
check("维修中灯具(7)预检失败", not body["ok"] and any("维修" in x or "repair" in x for x in body["errors"]), str(body.get("errors")))

print("\n=== 7. 模拟接口 ===")
st, pw = call("GET", "/power/live?studio_id=1", 3)
check("功率读数接口返回各回路", st==200 and len(pw["readings"])==3)
st, ns = call("GET", "/notifications", 1)
check("摄影师收到通知（提交/确认/取消/豁免）", st==200 and len(ns)>=3, f"{len(ns)} 条")

print(f"\n{'='*50}\n通过 {len(PASS)} 项，失败 {len(FAIL)} 项")
if FAIL:
    print("失败项:", FAIL); sys.exit(1)
print("🎉 全部核心不变量验证通过")
