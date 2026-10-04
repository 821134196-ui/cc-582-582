<script>
  import { onMount, tick } from 'svelte';
  import {
    api, STATUS_LABEL, RESULT_LABEL, fmtTime,
    toLocalInput, fromLocalInput,
  } from './api.js';
  import FloorPlan from './FloorPlan.svelte';

  export let projectId;
  export let meta;
  export let user;
  export let on_back;

  let p = null;
  let error = '';
  let info = '';

  // 编辑态
  let draftLights = [];
  let selectedLight = -1;
  let header = { name: '', venue_id: 1, start: '', end: '' };
  let note = '';
  let viewVersion = -1; // -1 = 最新（可编辑的基础）
  let recheck = null;

  // 归还
  let damageOf = null; // fixture_id 正在填写损坏说明
  let damageNote = '';

  // 功率
  let power = null;

  // 场务备注
  let remark = '';

  const isPhotographer = () => user.role === 'photographer';
  const ownsProject = () => p && user.id === p.photographer_id;
  const editable = () => p && ['draft', 'submitted'].includes(p.status);
  const confirmed = () => p && ['confirmed', 'returning'].includes(p.status);

  onMount(load);

  async function load() {
    p = await api('GET', `/projects/${projectId}`);
    header = {
      name: p.name,
      venue_id: p.venue_id ?? meta.venues[0].id,
      start: toLocalInput(p.shoot_start),
      end: toLocalInput(p.shoot_end),
    };
    const latest = latestLights();
    draftLights = latest.map((l) => ({ ...l }));
    viewVersion = p.versions[0]?.version ?? -1;
    selectedLight = -1;
    recheck = null;
    power = null;
  }

  function latestLights() {
    if (!p?.versions?.length) return [];
    try { return JSON.parse(p.versions[0].lights_json); } catch { return []; }
  }
  $: viewLights = (() => {
    if (!p) return [];
    if (viewVersion < 0) return draftLights;
    const v = p.versions.find((x) => x.version === viewVersion);
    return v ? JSON.parse(v.lights_json) : [];
  })();

  $: venue = meta.venues.find((v) => v.id === header.venue_id) ?? meta.venues[0];
  $: circuits = meta.circuits.filter((c) => c.venue_id === header.venue_id);
  $: returnedIds = p ? new Set(p.returns.map((r) => r.fixture_id)) : new Set();
  $: pendingFixtures = p ? p.booked.filter((b) => !returnedIds.has(b.fixture_id)) : [];

  // 本地估算各回路功率（仅本方案）
  $: localLoads = circuits.map((c) => {
    const added = draftLights.filter((l) => l.circuit_id === c.id).reduce((s, l) => s + (l.watts || 0), 0);
    return { c, added, over: added > c.max_watts };
  });

  function fixtureById(id) {
    return meta.fixtures.find((f) => f.id === id);
  }

  function addLight() {
    draftLights = [...draftLights, {
      fixture_id: meta.fixtures[0].id,
      circuit_id: circuits[0]?.id,
      watts: meta.fixtures[0].power_watts,
      pos_x: +(venue.width_m / 2).toFixed(2),
      pos_y: +(venue.height_m / 2).toFixed(2),
      label: `灯 ${draftLights.length + 1}`,
    }];
    selectedLight = draftLights.length - 1;
  }
  function removeLight(i) {
    draftLights = draftLights.filter((_, j) => j !== i);
    selectedLight = Math.min(selectedLight, draftLights.length - 1);
  }
  function onFixtureChange(i) {
    const f = fixtureById(draftLights[i].fixture_id);
    if (f) draftLights[i].watts = f.power_watts;
    draftLights = draftLights;
  }

  async function run(action, msg) {
    error = ''; info = '';
    try {
      await action();
      info = msg;
      await load();
    } catch (e) {
      error = e.message;
    }
  }

  const saveHeader = () => run(async () => {
    await api('PATCH', `/projects/${projectId}`, {
      name: header.name,
      venue_id: header.venue_id,
      start: fromLocalInput(header.start),
      end: fromLocalInput(header.end),
    });
  }, '项目信息已更新');

  const saveVersion = () => run(async () => {
    await api('POST', `/projects/${projectId}/versions`, { note, lights: draftLights });
    note = '';
  }, '新版本已保存（历史版本不会被覆盖）');

  const submitPlan = () => run(async () => {
    await api('POST', `/projects/${projectId}/submit`);
  }, '方案已提交，等待场务确认');

  async function precheck() {
    error = ''; info = '';
    try {
      recheck = await api('POST', `/projects/${projectId}/check`, {
        venue_id: header.venue_id,
        start: fromLocalInput(header.start),
        end: fromLocalInput(header.end),
        lights: latestLights(),
      });
      if (recheck.ok) info = '预检通过：无冲突、回路负载均在上限内';
    } catch (e) {
      error = e.message;
    }
  }

  const confirmPlan = () => run(async () => {
    await api('POST', `/projects/${projectId}/confirm`, { remark });
    remark = '';
  }, '排期已确认：场地与灯具同时占用');

  const rejectPlan = () => run(async () => {
    await api('POST', `/projects/${projectId}/reject`, { remark });
    remark = '';
  }, '方案已驳回');

  const replan = () => run(async () => {
    await api('POST', `/projects/${projectId}/replan`, {
      venue_id: header.venue_id,
      start: fromLocalInput(header.start),
      end: fromLocalInput(header.end),
      note,
      lights: draftLights,
    });
    note = '';
  }, '变更已提交：旧占用立即释放，等待场务重新确认');

  const cancelProject = () => {
    if (!confirm('确定取消该项目？将立即释放全部场地与灯具占用。')) return;
    run(async () => {
      await api('POST', `/projects/${projectId}/cancel`);
    }, '项目已取消，全部资源立即恢复可用');
  };

  async function returnOne(fid, condition) {
    let n = '';
    if (condition === 'damaged') {
      n = damageNote.trim();
      if (!n) { error = '请先填写损坏情况说明'; return; }
    }
    error = ''; info = '';
    try {
      const r = await api('POST', `/projects/${projectId}/returns`, {
        items: [{ fixture_id: fid, condition, note: n }],
      });
      info = condition === 'damaged'
        ? `已登记损坏并释放该灯具，剩余 ${r.remaining} 件未归还`
        : `已归还，剩余 ${r.remaining} 件未归还`;
      damageOf = null; damageNote = '';
      await load();
    } catch (e) { error = e.message; }
  }

  async function decide(did, waiver) {
    error = ''; info = '';
    try {
      await api('POST', `/damages/${did}/decision`, { waiver });
      info = `豁免已${waiver === 'approved' ? '批准' : '驳回'}`;
      await load();
    } catch (e) { error = e.message; }
  }

  async function simPower() {
    error = '';
    try { power = await api('GET', `/projects/${projectId}/sim-power`); }
    catch (e) { error = e.message; }
  }
</script>

{#if p}
  <div class="row" style="justify-content:space-between;">
    <button on:click={on_back}>← 返回项目列表</button>
    <span class="tag {p.status}" style="font-size:13px;">{STATUS_LABEL[p.status]}</span>
  </div>

  {#if error}<div class="err-box">⚠ {error}</div>{/if}
  {#if info}<div class="ok-box">✓ {info}</div>{/if}

  <!-- 项目信息 -->
  <div class="panel">
    <h2>{p.name}</h2>
    <p class="muted small">
      摄影师：{p.photographer.name} · 方案 v{p.versions[0]?.version ?? 0} ·
      创建 {fmtTime(p.created_at)}
    </p>
    <div class="grid2">
      <label>项目名称
        <input bind:value={header.name} disabled={!ownsProject() || !editable()} />
      </label>
      <label>场地
        <select bind:value={header.venue_id} disabled={!ownsProject() || (!editable() && !confirmed())}>
          {#each meta.venues as v}<option value={v.id}>{v.name}（{v.width_m}m×{v.height_m}m）</option>{/each}
        </select>
      </label>
      <label>开始时间
        <input type="datetime-local" bind:value={header.start}
               disabled={!ownsProject() || (!editable() && !confirmed())} />
      </label>
      <label>结束时间
        <input type="datetime-local" bind:value={header.end}
               disabled={!ownsProject() || (!editable() && !confirmed())} />
      </label>
    </div>
    <div style="margin-top:10px;">
      {#if ownsProject() && editable()}
        <button on:click={saveHeader}>保存项目信息</button>
      {/if}
    </div>
  </div>

  <!-- 布光设计（草稿/待确认：摄影师编辑；已确认：变更时编辑） -->
  <div class="panel">
    <h3>灯位平面图与器材清单</h3>
    <div class="row" style="margin-bottom:10px;">
      <label class="small muted">查看版本
        <select style="width:auto;display:inline-block;margin-left:6px;" bind:value={viewVersion}>
          <option value={-1}>{confirmed() ? '变更草稿（最新为基础）' : '编辑草稿（最新为基础）'}</option>
          {#each p.versions as v}
            <option value={v.version}>v{v.version} · {fmtTime(v.created_at)}{v.note ? ` · ${v.note}` : ''}</option>
          {/each}
        </select>
      </label>
      {#if !editable() && !confirmed()}<span class="muted small">（历史版本只读）</span>{/if}
    </div>

    <FloorPlan
      venue={venue}
      lights={viewVersion < 0 ? draftLights : viewLights}
      fixtures={meta.fixtures}
      readonly={viewVersion >= 0 || !ownsProject() || (!editable() && !confirmed())}
      bind:selected={selectedLight}
    />

    <!-- 器材清单 -->
    <table style="margin-top:12px;">
      <thead>
        <tr><th>#</th><th>灯具（实物）</th><th>回路</th><th>功率 W</th><th>标签</th><th>坐标 (m)</th><th></th></tr>
      </thead>
      <tbody>
        {#each viewVersion < 0 ? draftLights : viewLights as l, i}
          <tr class:sel={i === selectedLight && viewVersion < 0} on:click={() => viewVersion < 0 && (selectedLight = i)}>
            <td>{i + 1}</td>
            <td>
              {#if viewVersion < 0 && ownsProject() && (editable() || confirmed())}
                <select bind:value={l.fixture_id} on:change={() => onFixtureChange(i)}>
                  {#each meta.fixtures as f}
                    <option value={f.id}>{f.code} · {f.model}（{f.power_watts}W）</option>
                  {/each}
                </select>
              {:else}
                {fixtureById(l.fixture_id)?.code ?? '待定'} {fixtureById(l.fixture_id)?.model ?? ''}
              {/if}
            </td>
            <td>
              {#if viewVersion < 0 && ownsProject() && (editable() || confirmed())}
                <select bind:value={l.circuit_id}>
                  {#each circuits as c}<option value={c.id}>{c.name}（上限 {c.max_watts}W）</option>{/each}
                </select>
              {:else}
                {meta.circuits.find(c => c.id === l.circuit_id)?.name ?? l.circuit_id}
              {/if}
            </td>
            <td style="min-width:90px;">
              {#if viewVersion < 0 && ownsProject() && (editable() || confirmed())}
                <input type="number" bind:value={l.watts} min="1" style="width:80px;" />
              {:else}{l.watts}{/if}
            </td>
            <td>
              {#if viewVersion < 0 && ownsProject() && (editable() || confirmed())}
                <input bind:value={l.label} style="width:110px;" />
              {:else}{l.label}{/if}
            </td>
            <td class="small muted">{l.pos_x.toFixed(2)}, {l.pos_y.toFixed(2)}</td>
            <td>
              {#if viewVersion < 0 && ownsProject() && (editable() || confirmed())}
                <a on:click|stopPropagation={() => removeLight(i)}>删除</a>
              {/if}
            </td>
          </tr>
        {/each}
        {#if (viewVersion < 0 ? draftLights : viewLights).length === 0}
          <tr><td colspan="7" class="muted" style="text-align:center;">尚未添加灯位</td></tr>
        {/if}
      </tbody>
    </table>

    {#if viewVersion < 0 && ownsProject() && (editable() || confirmed())}
      <div class="row" style="margin-top:10px;">
        <button on:click={addLight}>+ 添加灯位</button>
        <input style="flex:1;min-width:200px;" placeholder="本版修改说明（可选）" bind:value={note} />
      </div>
    {/if}

    <!-- 回路功率预估 -->
    <div style="margin-top:14px;">
      <h3>回路负载预估（本方案）</h3>
      {#each localLoads.filter(x => x.added > 0 || editable() || confirmed()) as {c, added, over}}
        <div style="margin:6px 0;">
          <div class="row small" style="justify-content:space-between;">
            <strong>回路 {c.name}</strong>
            <span class:bad-text={over}>{added} / {c.max_watts} W {#if over}<strong>超限！</strong>{/if}</span>
          </div>
          <div class="bar"><i class:over={over} style="width:{Math.min(100, added / c.max_watts * 100)}%"></i></div>
        </div>
      {/each}
    </div>
  </div>

  <!-- 摄影师操作 -->
  {#if ownsProject()}
    <div class="panel">
      <h3>摄影师操作</h3>
      <div class="row">
        {#if editable()}
          <button class="primary" on:click={saveVersion}>保存新版本</button>
          <button class="ok" on:click={submitPlan} disabled={p.versions.length === 0}>提交场务确认</button>
        {/if}
        {#if confirmed()}
          <button class="warn" on:click={replan}>换灯 / 延时：变更方案并重新确认</button>
          <span class="muted small">提交后旧占用立即释放，需场务重新确认后才会再次占用</span>
        {/if}
        {#if !['completed', 'cancelled'].includes(p.status)}
          <button class="danger" on:click={cancelProject}>取消项目（释放全部资源）</button>
        {/if}
      </div>
    </div>
  {/if}

  <!-- 场务审核 -->
  {#if user.role === 'crew' && p.status === 'submitted'}
    <div class="panel">
      <h3>场务审核</h3>
      <div class="row">
        <button on:click={precheck}>🔍 核对实物 / 场地 / 回路负载（预检）</button>
        <input style="flex:1;min-width:220px;" placeholder="确认或驳回备注（可选）" bind:value={remark} />
      </div>

      {#if recheck}
        <div style="margin-top:12px;">
          {#if recheck.venue_busy}
            <div class="err-box">场地与项目「{recheck.venue_busy.name}」时段冲突</div>
          {/if}
          {#each recheck.fixture_busy as fb}
            <div class="err-box">灯具 #{fb.fixture_id} 与「{fb.project.name}」冲突：{fmtTime(fb.start)} ~ {fmtTime(fb.end)}</div>
          {/each}
          <h3 style="margin-top:10px;">各回路负载（含同时段其他项目）</h3>
          {#each recheck.circuit_loads as l}
            <div style="margin:6px 0;">
              <div class="row small" style="justify-content:space-between;">
                <strong>回路 {l.circuit_name}</strong>
                <span class:bad-text={l.overload}>
                  已有 {l.used_watts}W + 本方案 {l.added_watts}W = {l.total_watts}W / 上限 {l.max_watts}W
                  {#if l.overload}<strong>超限，系统禁止确认</strong>{/if}
                </span>
              </div>
              <div class="bar"><i class:over={l.overload} style="width:{Math.min(100, l.total_watts / l.max_watts * 100)}%"></i></div>
            </div>
          {/each}
        </div>
      {/if}

      <div class="row" style="margin-top:12px;">
        <button class="ok" on:click={confirmPlan}>✓ 确认排期（同时占用场地与灯具）</button>
        <button class="danger" on:click={rejectPlan}>驳回方案</button>
      </div>
      <p class="muted small" style="margin-bottom:0;">
        功率超限、灯具互斥或场地冲突时，服务端直接拒绝，不存在手工放行通道。
      </p>
    </div>
  {/if}

  <!-- 归还登记 -->
  {#if user.role === 'crew' && confirmed()}
    <div class="panel">
      <h3>逐件归还登记</h3>
      <p class="muted small">已归还 {p.returns.length} / {p.booked.length} 件；单件归还后该灯具立即恢复可借。</p>
      <table>
        <thead><tr><th>灯具</th><th>型号</th><th>回路</th><th>功率</th><th>状态</th><th>操作</th></tr></thead>
        <tbody>
          {#each p.booked as b}
            <tr>
              <td>{fixtureById(b.fixture_id)?.code}</td>
              <td>{fixtureById(b.fixture_id)?.model}</td>
              <td>{meta.circuits.find(c => c.id === b.circuit_id)?.name}</td>
              <td>{b.watts}W</td>
              <td>
                {#if returnedIds.has(b.fixture_id)}
                  {#if p.returns.find(x => x.fixture_id === b.fixture_id).condition === 'ok'}<span style="color:var(--ok);">已完好归还</span>
                  {:else}<span style="color:var(--bad);">损坏归还</span>{/if}
                {:else}<span class="tag confirmed">占用中</span>{/if}
              </td>
              <td>
                {#if !returnedIds.has(b.fixture_id)}
                  {#if damageOf === b.fixture_id}
                    <input style="width:180px;" placeholder="损坏情况说明" bind:value={damageNote} />
                    <button class="danger" on:click={() => returnOne(b.fixture_id, 'damaged')}>确认损坏登记</button>
                    <button on:click={() => (damageOf = null)}>取消</button>
                  {:else}
                    <button class="ok" on:click={() => returnOne(b.fixture_id, 'ok')}>完好归还</button>
                    <button class="warn" on:click={() => (damageOf = b.fixture_id, damageNote = '')}>损坏归还</button>
                  {/if}
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}

  <!-- 损坏记录与豁免 -->
  {#if p.damages.length > 0}
    <div class="panel">
      <h3>损坏记录与豁免审批（关联本项目）</h3>
      <table>
        <thead><tr><th>灯具</th><th>情况说明</th><th>豁免状态</th><th>审批人</th><th>操作</th></tr></thead>
        <tbody>
          {#each p.damages as d}
            <tr>
              <td>{d.fixture_code}</td>
              <td>{d.description || '—'}</td>
              <td>
                {#if d.waiver === 'pending'}<span class="tag submitted">待审批</span>
                {:else if d.waiver === 'approved'}<span class="tag confirmed-res">已批准</span>
                {:else}<span class="tag rejected-res">已驳回</span>{/if}
              </td>
              <td>{d.decided_by_name ?? '—'}</td>
              <td>
                {#if user.role === 'crew' && d.waiver === 'pending'}
                  <button class="ok" on:click={() => decide(d.id, 'approved')}>批准豁免</button>
                  <button class="danger" on:click={() => decide(d.id, 'denied')}>驳回</button>
                {/if}
                {#if user.role === 'photographer'}
                  <span class="muted small">摄影师无权审批损坏豁免</span>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}

  <!-- 模拟功率读数 -->
  {#if confirmed()}
    <div class="panel">
      <h3>实时功率读数（本地模拟电表）</h3>
      <button on:click={simPower}>读取各回路实时功率</button>
      {#if power}
        {#if power.overload}
          <div class="err-box">检测到回路实时过载！已推送通知。</div>
        {:else}
          <div class="ok-box">各回路实时功率均在安全范围。</div>
        {/if}
        {#each power.readings as r}
          <div style="margin:6px 0;">
            <div class="row small" style="justify-content:space-between;">
              <strong>{r.circuit_name}</strong>
              <span class:bad-text={r.overload}>实测 {r.measured_watts}W（额定 {r.nominal_watts}W）/ {r.max_watts}W</span>
            </div>
            <div class="bar"><i class:over={r.overload} style="width:{Math.min(100, r.measured_watts / r.max_watts * 100)}%"></i></div>
          </div>
        {/each}
      {/if}
    </div>
  {/if}

  <!-- 确认历史与版本 -->
  <div class="grid2">
    <div class="panel">
      <h3>方案版本</h3>
      {#each p.versions as v}
        <div style="padding:8px 0;border-bottom:1px solid var(--line);">
          <strong>v{v.version}</strong>
          <span class="muted small"> · {fmtTime(v.created_at)}</span>
          {#if v.note}<div class="small">{v.note}</div>{/if}
          <div class="muted small">{JSON.parse(v.lights_json).length} 盏灯</div>
        </div>
      {/each}
      {#if p.versions.length === 0}<p class="muted small">尚未保存任何版本</p>{/if}
    </div>
    <div class="panel">
      <h3>确认 / 重新确认记录</h3>
      {#each p.confirmations as c}
        <div style="padding:8px 0;border-bottom:1px solid var(--line);">
          <span class="tag {c.result}-res">{RESULT_LABEL[c.result]}</span>
          <span class="muted small"> · v{p.versions.find(v => v.id === c.plan_version_id)?.version ?? '?'} · {c.crew_name} · {fmtTime(c.created_at)}</span>
          {#if c.remark}<div class="small">{c.remark}</div>{/if}
        </div>
      {/each}
      {#if p.confirmations.length === 0}<p class="muted small">暂无确认记录</p>{/if}
    </div>
  </div>
{/if}

<style>
  tr.sel { background: rgba(79,140,255,.12); }
  .bad-text { color: var(--bad); font-weight: 600; }
</style>
