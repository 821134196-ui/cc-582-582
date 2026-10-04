#!/usr/bin/env python3
"""
端到端验证脚本（启动独立测试库 + 真实 HTTP 请求）：
  1. 同一灯具不能被两组同时占用（含跨场地）
  2. 回路功率超限不得确认，force=true 手工放行也被拒绝
  3. 取消项目后场地与灯具立即恢复可用
  4. 摄影师不能审批自己项目的损坏豁免
  5. 方案修改保留版本、旧占用释放、重新确认结果可追溯
  6. 逐件归还立即释放灯具，损坏记录关联项目
"""
import json
import os
import signal
import sqlite3
import subprocess
import sys
import time
import urllib.request
import urllib.error

BASE = "http://127.0.0.1:8091/api"
DB = "/tmp/sls_verify.db"
BIN = "/workspace/target/debug/studio-light-scheduler"

PHOTO1, PHOTO2, CREW = 1, 2, 3  # 陈曦 / 林夏 / 老周（种子数据）
VENUE1, VENUE2 = 1, 2
# 回路
A1, A2, B1 = 1, 2, 4
# 灯具
LED1, LED2 = 1, 2          # LED-300-01 / 02 (300W)
LED500_1 = 3               # 500W
TUNG1 = 5                  # 1000W
STROBE1 = 7                # 600W

T1, T2 = "2026-11-01T10:00:00Z", "2026-11-01T13:00:00Z"
T3, T4 = "2026-11-01T14:00:00Z", "2026-11-01T16:00:00Z"

PASS, FAILS = 0, []


def check(name, cond, detail=""):
    global PASS
    if cond:
        PASS += 1
        print(f"  PASS  {name}")
    else:
        FAILS.append((name, detail))
        print(f"  FAIL  {name}  {detail}")


def req(method, path, body=None, uid=None):
    data = json.dumps(body).encode() if body is not None else None
    r = urllib.request.Request(BASE + path, data=data, method=method)
    r.add_header("Content-Type", "application/json")
    if uid:
        r.add_header("X-User-Id", str(uid))
    try:
        with urllib.request.urlopen(r) as resp:
            return resp.status, json.loads(resp.read() or b"{}")
    except urllib.error.HTTPError as e:
        return e.code, json.loads(e.read() or b"{}")


def light(fid, cid, watts, x=2.0, y=2.0, label=""):
    return {"fixture_id": fid, "circuit_id": cid, "watts": watts,
            "pos_x": x, "pos_y": y, "label": label}


def create_project(uid, name, venue, start, end):
    s, r = req("POST", "/projects", {"name": name, "venue_id": venue, "start": start, "end": end}, uid)
    assert s == 200, (s, r)
    return r["id"]


def save(pid, uid, lights, note=""):
    s, r = req("POST", f"/projects/{pid}/versions", {"note": note, "lights": lights}, uid)
    assert s == 200, (s, r)
    return r["version"]


def submit(pid, uid):
    return req("POST", f"/projects/{pid}/submit", None, uid)


def get(pid):
    s, r = req("GET", f"/projects/{pid}")
    assert s == 200, (s, r)
    return r


def wait_health(proc):
    for _ in range(50):
        try:
            with urllib.request.urlopen(BASE + "/health") as r:
                if r.status == 200:
                    return
        except OSError:
            time.sleep(0.1)
    raise RuntimeError("server failed to start")


def main():
    for ext in ("", "-wal", "-shm"):
        try:
            os.remove(DB + ext)
        except FileNotFoundError:
            pass

    env = dict(os.environ, DATABASE_URL=f"sqlite:{DB}?mode=rwc", PORT="8091")
    proc = subprocess.Popen([BIN], env=env, cwd="/workspace",
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        wait_health(proc)
        run_tests()
    finally:
        proc.send_signal(signal.SIGINT)
        proc.wait(timeout=5)

    print(f"\n{PASS} passed, {len(FAILS)} failed")
    if FAILS:
        for n, d in FAILS:
            print(" -", n, d)
        sys.exit(1)


def run_tests():
    print("\n[1] 基础流程：P1 提交 -> 预检 -> 场务确认，场地+灯具同时占用")
    p1 = create_project(PHOTO1, "秋装硬照 P1", VENUE1, T1, T2)
    v1 = save(p1, PHOTO1, [light(LED1, A1, 300, 3, 3, "主光")])
    check("保存产生版本 v1", v1 == 1)
    s, _ = submit(p1, PHOTO1)
    check("摄影师提交", s == 200)
    s, r = req("POST", f"/projects/{p1}/check",
               {"venue_id": VENUE1, "start": T1, "end": T2, "lights": [light(LED1, A1, 300)]})
    check("场务预检通过", s == 200 and r["ok"] is True and r["errors"] == [], r)
    s, r = req("POST", f"/projects/{p1}/confirm", {"remark": "实物核对无误"}, CREW)
    check("场务确认成功", s == 200, r)
    d = get(p1)
    check("状态=confirmed", d["status"] == "confirmed", d["status"])
    check("确认记录落库", len(d["confirmations"]) == 1 and d["confirmations"][0]["result"] == "confirmed")
    check("已生成灯位/场地/灯具占用", len(d["booked"]) == 1 and d["venue_slot"] and len(d["fixture_slots"]) == 1)

    print("\n[2] 设备互斥：P2 同时段（跨二号棚）使用同一台 LED-300-01")
    p2 = create_project(PHOTO2, "潮牌大片 P2", VENUE2, T1, T2)
    save(p2, PHOTO2, [light(LED1, B1, 300)])
    submit(p2, PHOTO2)
    s, r = req("POST", f"/projects/{p2}/check",
               {"venue_id": VENUE2, "start": T1, "end": T2, "lights": [light(LED1, B1, 300)]})
    check("预检报告灯具冲突", s == 200 and r["ok"] is False
          and any("灯具" in e for e in r["errors"]), r)
    check("fixture_busy 指向 P1", len(r["fixture_busy"]) == 1 and r["fixture_busy"][0]["project"]["id"] == p1, r)
    s, r = req("POST", f"/projects/{p2}/confirm", {}, CREW)
    check("场务确认被拒 409", s == 409 and "冲突" in r["error"], (s, r))
    check("冲突后 P2 仍为 submitted（未产生任何占用）", get(p2)["status"] == "submitted")

    print("\n[3] 回路阈值：500+1000+600=2100W > B1 上限 2000W")
    over_lights = [light(LED500_1, B1, 500), light(TUNG1, B1, 1000, 4, 4), light(STROBE1, B1, 600, 5, 5)]
    v2 = save(p2, PHOTO2, over_lights, "换成大功率组合")
    check("超限修改保留为新版本 v2", v2 == 2)
    s, r = req("POST", f"/projects/{p2}/check",
               {"venue_id": VENUE2, "start": T1, "end": T2, "lights": over_lights})
    check("预检标记回路过载", s == 200 and r["ok"] is False
          and any(l["overload"] for l in r["circuit_loads"])
          and any("超限" in e for e in r["errors"]), r)
    s, r = req("POST", f"/projects/{p2}/confirm", {}, CREW)
    check("超限确认被拒 409", s == 409 and "超限" in r["error"], (s, r))
    s, r = req("POST", f"/projects/{p2}/confirm", {"force": True}, CREW)
    check("force=true 手工放行同样被拒（403）", s == 403 and "强制放行" in r["error"], (s, r))
    # 直接打数据库也证明没有半成品占用
    con = sqlite3.connect(DB)
    n = con.execute("SELECT COUNT(*) FROM fixture_slots WHERE project_id=?", (p2,)).fetchone()[0]
    check("被拒后数据库无 P2 的灯具占用", n == 0, n)
    con.close()

    print("\n[4] 取消即释放：P3 场地与 P1 冲突，取消 P1 后 P3 立即确认成功")
    p3 = create_project(PHOTO1, "杂志封面 P3", VENUE1, T1, T2)
    save(p3, PHOTO1, [light(LED2, A1, 300, 6, 6)])
    submit(p3, PHOTO1)
    s, r = req("POST", f"/projects/{p3}/confirm", {}, CREW)
    check("取消前：场地冲突，P3 确认失败", s == 409 and "场地" in r["error"], (s, r))
    s, _ = req("POST", f"/projects/{p1}/cancel", None, PHOTO1)
    check("摄影师取消自己的 P1", s == 200, s)
    check("P1 状态=cancelled", get(p1)["status"] == "cancelled")
    s, _ = req("POST", f"/projects/{p3}/confirm", {}, CREW)
    check("取消后立即确认 P3 成功（场地恢复）", s == 200, s)
    # P1 用过的 LED1 也应立即可用：P5 同时段在二号棚占用 LED1
    p5 = create_project(PHOTO2, "广告短片 P5", VENUE2, T1, T2)
    save(p5, PHOTO2, [light(LED1, B1, 300)])
    submit(p5, PHOTO2)
    s, _ = req("POST", f"/projects/{p5}/confirm", {}, CREW)
    check("P1 释放的 LED-300-01 立即被 P5 占用成功", s == 200, s)

    print("\n[5] 逐件归还 + 损坏记录 + 豁免权限")
    s, r = req("POST", f"/projects/{p3}/returns",
               {"items": [{"fixture_id": LED2, "condition": "damaged", "note": "灯脚摔变形"}]}, CREW)
    check("场务登记损坏归还", s == 200 and r["status"] == "completed", (s, r))
    d = get(p3)
    check("损坏记录关联本次项目", len(d["damages"]) == 1 and d["damages"][0]["fixture_id"] == LED2
          and d["damages"][0]["waiver"] == "pending")
    s, tl = req("GET", "/timeline")
    check("归还后该灯具时段立即消失",
          not any(x["kind"] == "fixture" and x["ref_id"] == LED2 for x in tl["fixture_slots"]))
    did = d["damages"][0]["id"]
    s, r = req("POST", f"/damages/{did}/decision", {"waiver": "approved"}, PHOTO1)
    check("摄影师不能审批自己项目的损坏豁免（403）", s == 403, (s, r))
    s, r = req("POST", f"/damages/{did}/decision", {"waiver": "approved"}, PHOTO2)
    check("其他摄影师同样无权审批（403）", s == 403, (s, r))
    s, _ = req("POST", f"/damages/{did}/decision", {"waiver": "approved"}, CREW)
    check("场务批准豁免成功", s == 200, s)
    check("豁免结果已记录审批人", get(p3)["damages"][0]["decided_by"] == CREW)

    print("\n[6] 换灯/延时：重新计算占用与功率，旧占用释放，版本与重新确认可追溯")
    p4 = create_project(PHOTO1, "静物特写 P4", VENUE1, T3, T4)
    save(p4, PHOTO1, [light(LED2, A1, 300, 2, 2)])
    submit(p4, PHOTO1)
    s, _ = req("POST", f"/projects/{p4}/confirm", {}, CREW)
    check("P4 首次确认", s == 200, s)
    # 延时到与 P5 同时段并改用 LED1（LED1 此时被 P5 占用到 13:00；14:00 之后不冲突，这里只测版本流程）
    new_lights = [light(LED2, A1, 300, 2, 2, "主光"), light(LED500_1, A2, 500, 8, 5, "轮廓光")]
    s, r = req("POST", f"/projects/{p4}/replan",
               {"venue_id": VENUE1, "start": T3, "end": "2026-11-01T17:00:00Z",
                "note": "延时并加一盏轮廓光", "lights": new_lights}, PHOTO1)
    check("提交变更方案", s == 200 and r["version"] == 2, (s, r))
    d = get(p4)
    check("变更后回到待确认", d["status"] == "submitted", d["status"])
    check("旧版本 v1 保留、新增 v2",
          len(d["versions"]) == 2 and [v["version"] for v in d["versions"]] == [2, 1])
    check("旧占用已立即释放（无 slot）", d["venue_slot"] is None and len(d["fixture_slots"]) == 0)
    check("确认历史含 revoked 记录", any(c["result"] == "revoked" for c in d["confirmations"]))
    s, _ = req("POST", f"/projects/{p4}/confirm", {"remark": "新方案复核通过"}, CREW)
    check("场务按 v2 重新确认成功", s == 200, s)
    d = get(p4)
    check("重新确认后占用 2 盏灯", len(d["fixture_slots"]) == 2 and d["venue_slot"] is not None)
    check("确认历史保留两次 confirmed + 一次 revoked",
          [c["result"] for c in d["confirmations"]].count("confirmed") == 2
          and any(c["result"] == "revoked" for c in d["confirmations"]))

    print("\n[7] 变更时若新方案超限，则旧占用保留、不会被错误释放")
    bad_lights = [light(LED2, A1, 1000), light(TUNG1, A1, 1000), light(STROBE1, A1, 600)]
    s, r = req("POST", f"/projects/{p4}/replan",
               {"venue_id": VENUE1, "start": T3, "end": "2026-11-01T17:00:00Z",
                "note": "恶意超限", "lights": bad_lights}, PHOTO1)
    check("超限的变更被拒绝", s == 409 and "超限" in r["error"], (s, r))
    d = get(p4)
    check("拒绝后项目仍 confirmed 且旧占用保留",
          d["status"] == "confirmed" and d["venue_slot"] is not None and len(d["fixture_slots"]) == 2,
          d["status"])

    print("\n[8] 其他规则")
    # 摄影师无权确认
    s, r = req("POST", f"/projects/{p2}/confirm", {}, PHOTO2)
    check("摄影师不能确认排期（403）", s == 403, (s, r))
    # 模拟功率读数
    s, r = req("GET", f"/projects/{p4}/sim-power")
    check("模拟功率读数接口可用", s == 200 and "readings" in r, (s, r))
    # 时间相邻不算冲突（13:00 结束 vs 13:00 开始）
    s, r = req("POST", f"/projects/{p2}/check",
               {"venue_id": VENUE2, "start": T2, "end": "2026-11-01T14:00:00Z",
                "lights": [light(LED2, B1, 300)]})
    check("首尾相接的时段允许（半开区间）", s == 200 and r["ok"] is True, r)
    # 通知已产生
    s, notes = req("GET", "/notifications", uid=CREW)
    check("通知接口可查询", s == 200 and len(notes) >= 3, s)

    print("\n[9] 并发强互斥：两个场务请求同时确认同一灯具，恰好一个成功")
    import http.client
    import threading as _th

    def raw9(method, path, body=None):
        conn = http.client.HTTPConnection("127.0.0.1", 8091, timeout=10)
        payload = json.dumps(body) if body is not None else None
        headers = {"Content-Type": "application/json", "X-User-Id": str(CREW)}
        conn.request(method, "/api" + path, body=payload, headers=headers)
        r = conn.getresponse()
        out = (r.status, json.loads(r.read() or b"{}"))
        conn.close()
        return out

    cT, cT2 = "2026-12-01T10:00:00Z", "2026-12-01T13:00:00Z"
    cids = []
    for uid in (PHOTO1, PHOTO2):
        pid = create_project(uid, f"并发项目{uid}", VENUE1, cT, cT2)
        save(pid, uid, [light(LED1, A1, 300)])
        submit(pid, uid)
        cids.append(pid)

    results9 = []
    barrier = _th.Barrier(2)
    def worker9(pid):
        barrier.wait()
        results9.append(raw9("POST", f"/projects/{pid}/confirm", {}))
    threads = [_th.Thread(target=worker9, args=(pid,)) for pid in cids]
    [t.start() for t in threads]
    [t.join() for t in threads]
    statuses9 = sorted(s for s, _ in results9)
    check("并发确认结果为 {200, 409}", statuses9 == [200, 409], statuses9)
    con = sqlite3.connect(DB)
    n = con.execute(
        "SELECT COUNT(*) FROM fixture_slots WHERE fixture_id=? AND start_at=? AND end_at=?",
        (LED1, cT, cT2),
    ).fetchone()[0]
    nv = con.execute("SELECT COUNT(*) FROM venue_slots WHERE start_at=? AND end_at=?", (cT, cT2)).fetchone()[0]
    check("数据库中该灯具/场地各只有一行占用", n == 1 and nv == 1, (n, nv))
    check("败者状态仍为 submitted、胜者为 confirmed",
          sorted(get(pid)["status"] for pid in cids) == ["confirmed", "submitted"])


if __name__ == "__main__":
    main()
