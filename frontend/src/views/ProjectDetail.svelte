<script>
  import { createEventDispatcher, onMount } from 'svelte';
  import { api, fmtTime } from '../api.js';
  import { currentUser, studios, STATUS_LABELS, userName, toast, loadStudios } from '../stores.js';
  import FloorPlan from '../components/FloorPlan.svelte';
  import ProjectForm from '../components/ProjectForm.svelte';
  import DamagesPanel from '../components/DamagesPanel.svelte';

  export let id;
  const dispatch = createEventDispatcher();

  let p = null;
  let report = null;
  let editing = false;
  let decisionNote = '';
  let returnFixture = null;
  let returnCondition = 'ok';
  let returnNote = '';
  let waiverNote = '';
  let busy = false;

  async function load() {
    p = await api(`/projects/${id}`);
    loadStudios();
    try {
      report = await api(`/projects/${id}/validate`);
    } catch (e) {
      report = null;
    }
  }
  onMount(load);

  $: studio = p ? $studios.find((s) => s.id === p.studio_id) : null;
  $: currentVersion = p?.versions.find((v) => v.id === p.current_version_id) || p?.versions[p.versions.length - 1];
  $: items = currentVersion?.plan?.items || [];
  $: isOwner = $currentUser?.id === p?.photographer_id;
  $: isCrew = $currentUser && ($currentUser.role === 'crew' || $currentUser.role === 'admin');

  async function submit() {
    busy = true;
    try {
      await api(`/projects/${id}/submit`, { method: 'POST' });
      toast('已提交场务确认', 'ok');
      await load();
    } catch (e) { toast(e.message, 'err'); } finally { busy = false; }
  }

  async function decide(decision) {
    busy = true;
    try {
      const r = await api(`/projects/${id}/confirm`, {
        method: 'POST',
        body: JSON.stringify({ decision, note: decisionNote || null }),
      });
      if (decision === 'confirmed') {
        toast('已确认：场地与灯具在同一事务内锁定', 'ok');
      } else {
        toast('处理完成（原资源未锁定/已释放）', 'ok');
      }
      report = r.report;
      decisionNote = '';
      await load();
    } catch (e) {
      // 冲突时后端返回详细 report
      report = e.report || report;
      toast(e.message, 'err', 8000);
    } finally { busy = false; }
  }

  async function cancel() {
    if (!confirm('确定取消该项目？全部灯具与场地占用将立即释放，其他项目可以立刻使用。')) return;
    busy = true;
    try {
      await api(`/projects/${id}/cancel`, { method: 'POST' });
      toast('项目已取消，全部资源立即恢复可用', 'ok');
      await load();
      dispatch('changed');
    } catch (e) { toast(e.message, 'err'); } finally { busy = false; }
  }

  async function startShoot() {
    busy = true;
    try { await api(`/projects/${id}/start`, { method: 'POST' }); toast('已开拍', 'ok'); await load(); }
    catch (e) { toast(e.message, 'err'); } finally { busy = false; }
  }

  async function completeShoot() {
    busy = true;
    try { await api(`/projects/${id}/complete`, { method: 'POST' }); toast('项目已完工，场地已释放', 'ok'); await load(); dispatch('changed'); }
    catch (e) { toast(e.message, 'err'); } finally { busy = false; }
  }

  async function submitReturn() {
    if (!returnFixture) return;
    busy = true;
    try {
      const r = await api(`/projects/${id}/returns`, {
        method: 'POST',
        body: JSON.stringify({ fixture_id: returnFixture, condition: returnCondition, note: returnNote || null }),
      });
      toast(returnCondition === 'damaged'
        ? `已登记归还并创建损坏记录 #${r.damage_id}（关联本项目）`
        : '归还登记完成，该灯具已恢复可用', 'ok');
      returnFixture = null; returnNote = ''; returnCondition = 'ok';
      await load();
      dispatch('changed');
    } catch (e) { toast(e.message, 'err'); } finally { busy = false; }
  }

  async function requestWaiver(did) {
    try {
      await api(`/damages/${did}/waiver/request`, { method: 'POST' });
      toast('豁免申请已提交，等待场务/他人审批（本人无权审批）', 'ok');
      await load();
    } catch (e) { toast(e.message, 'err'); }
  }

  async function waiverDecision(did, approve) {
    try {
      await api(`/damages/${did}/waiver/decision`, {
        method: 'POST',
        body: JSON.stringify({ approve, note: waiverNote || null }),
      });
      toast(approve ? '豁免已批准' : '豁免已驳回', 'ok');
      waiverNote = '';
      await load();
    } catch (e) { toast(e.message, 'err'); }
  }

  $: heldBookings = p?.bookings || [];
  function fixtureName(fid) { return studio?.fixtures.find((f) => f.id === fid)?.name || `灯#${fid}`; }
  function circuitName(cid) { return studio?.circuits.find((c) => c.id === cid)?.name || `回路#${cid}`; }
</script>

{#if !p}
  <div class="panel muted">加载中…</div>
{:else if editing}
  <ProjectForm project={p} on:done={() => { editing = false; load(); dispatch('changed'); }} />
{:else}
  <div class="grid">
    <div class="panel">
      <div class="row" style="justify-content:space-between">
        <div>
          <h2 style="margin:0">#{p.id} {p.name} <span class="badge {p.status}">{STATUS_LABELS[p.status]}</span></h2>
          <div class="muted small" style="margin-top:4px">
            {userName(p.photographer_id)} · {studio?.name} · {fmtTime(p.start_at)} ~ {fmtTime(p.end_at)}
          </div>
        </div>
        <div class="row">
          <button class="btn ghost" on:click={() => dispatch('close')}>← 返回列表</button>
          {#if ['draft','changes_requested','rejected','pending','confirmed'].includes(p.status) && (isOwner || $currentUser?.role === 'admin')}
            <button class="btn ghost" on:click={() => (editing = true)}>
              {p.status === 'confirmed' ? '修改（换灯/延时将释放占用并重新确认）' : '编辑方案（生成新版本）'}
            </button>
          {/if}
          <button class="btn danger" on:click={cancel} disabled={busy}>取消项目并释放全部资源</button>
        </div>
      </div>
    </div>

    <!-- 操作区 -->
    <div class="two-col">
      <div class="panel">
        <h2>灯位平面图（当前版本 v{currentVersion?.version_no}）</h2>
        <FloorPlan {studio} {items} fixtures={studio?.fixtures || []} editable={false} selected={-1} />
        <h3>本版本器材清单</h3>
        <table>
          <thead><tr><th>#</th><th>灯具</th><th>功率</th><th>回路</th><th>坐标(m)</th></tr></thead>
          <tbody>
            {#each items as it, i}
              <tr>
                <td>{i + 1}</td><td>{fixtureName(it.fixture_id)}</td>
                <td>{studio.fixtures.find((f) => f.id === it.fixture_id)?.power_w}W</td>
                <td>{circuitName(it.circuit_id)}</td>
                <td>{it.x ?? '—'}, {it.y ?? '—'}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>

      <div class="grid">
        <!-- 实时核对 -->
        <div class="panel">
          <h2>资源与功率核对</h2>
          {#if report}
            {#if report.ok}
              <div class="ok-box">✅ 当前可排期：无设备冲突、无回路超限</div>
            {:else}
              <div class="error-list">
                <strong>不可确认（{report.errors.length}）：</strong>
                <ul>{#each report.errors as er}<li>{er}</li>{/each}</ul>
              </div>
            {/if}
            <table style="margin-top:8px">
              <thead><tr><th>回路</th><th>既有</th><th>本方案</th><th>合计/上限</th><th>状态</th></tr></thead>
              <tbody>
                {#each report.circuit_loads as c}
                  <tr>
                    <td>{c.circuit_name}</td>
                    <td>{c.used_power_w}W</td>
                    <td>{c.planned_power_w}W</td>
                    <td class={c.overloaded ? '' : ''} style:color={c.overloaded ? 'var(--danger)' : 'var(--ok)'}>
                      {c.total_after_w}/{c.max_power_w}W
                    </td>
                    <td>{#if c.overloaded}<span class="badge rejected">超限</span>{:else}<span class="badge completed">余量{c.headroom_w}W</span>{/if}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          {/if}

          <div style="margin-top:12px" class="grid">
            {#if isOwner && ['draft','changes_requested','rejected'].includes(p.status)}
              <button class="btn" on:click={submit} disabled={busy || (report && !report.ok)}>提交场务确认</button>
            {/if}

            {#if isCrew && !isOwner && ['pending','draft','changes_requested','rejected'].includes(p.status)}
              <textarea placeholder="场务核对意见（驳回/要求修改时填写）" bind:value={decisionNote} rows="2"></textarea>
              <div class="row">
                <button class="btn ok" on:click={() => decide('confirmed')} disabled={busy || !report?.ok}>
                  ✅ 核对无误，确认排期（同时锁定场地+灯具）
                </button>
                <button class="btn ghost" on:click={() => decide('changes_requested')} disabled={busy}>要求修改</button>
                <button class="btn danger" on:click={() => decide('rejected')} disabled={busy}>驳回</button>
              </div>
              <p class="small muted" style="margin:4px 0 0">
                确认时系统重新计算设备互斥与各回路功率；超限或占用冲突时确认按钮禁用，且接口层与数据库层均无手工放行入口。
              </p>
            {/if}

            {#if isCrew && p.status === 'confirmed'}
              <button class="btn" on:click={startShoot} disabled={busy}>▶ 标记开拍（开始逐件归还流程）</button>
            {/if}
            {#if (isCrew || isOwner) && p.status === 'active'}
              <button class="btn ok" on:click={completeShoot} disabled={busy || heldBookings.length > 0}>
                {heldBookings.length > 0 ? `还有 ${heldBookings.length} 件未归还，不能完工` : '✔ 全部归还，完工并释放场地'}
              </button>
            {/if}
          </div>
        </div>

        <!-- 归还 -->
        <div class="panel">
          <h2>逐件归还登记</h2>
          {#if ['active','confirmed'].includes(p.status)}
            {#if heldBookings.length}
              <table>
                <thead><tr><th>灯具</th><th>回路</th><th>操作</th></tr></thead>
                <tbody>
                  {#each heldBookings as b}
                    <tr>
                      <td>{fixtureName(b.fixture_id)}</td>
                      <td class="small">{circuitName(b.circuit_id)}</td>
                      <td><button class="btn sm ghost" on:click={() => (returnFixture = b.fixture_id)}>登记归还</button></td>
                    </tr>
                  {/each}
                </tbody>
              </table>
              {#if returnFixture != null}
                <div class="panel" style="background:var(--panel-2);margin-top:10px">
                  <strong>归还：{fixtureName(returnFixture)}</strong>
                  <div class="row" style="margin:8px 0">
                    <label><input type="radio" bind:group={returnCondition} value="ok"> 完好</label>
                    <label><input type="radio" bind:group={returnCondition} value="damaged"> 损坏</label>
                  </div>
                  {#if returnCondition === 'damaged'}
                    <input type="text" placeholder="损坏情况描述" bind:value={returnNote} style="width:100%" />
                  {/if}
                  <div class="row" style="margin-top:8px;justify-content:flex-end">
                    <button class="btn ghost sm" on:click={() => (returnFixture = null)}>取消</button>
                    <button class="btn sm" on:click={submitReturn} disabled={busy || (returnCondition === 'damaged' && !returnNote.trim())}>确认归还</button>
                  </div>
                </div>
              {/if}
            {:else}
              <p class="ok-box">本项目灯具均已逐件归还，灯具即时恢复可被其他项目占用。</p>
            {/if}
          {:else}
            <p class="muted small">项目开拍后可在此逐件登记归还。</p>
          {/if}

          {#if p.returns.length}
            <h3>归还记录</h3>
            <table>
              <tbody>
                {#each p.returns as r}
                  <tr>
                    <td>{fixtureName(r.fixture_id)}</td>
                    <td>{#if r.condition === 'damaged'}<span class="badge rejected">损坏</span>{:else}<span class="badge completed">完好</span>{/if}</td>
                    <td class="small muted">{r.note || ''} · {fmtTime(r.created_at)}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          {/if}
        </div>
      </div>
    </div>

    <!-- 损坏与豁免 -->
    <div class="panel">
      <h2>损坏记录与豁免审批（关联本项目）</h2>
      <div id="damages-mount"></div>
      <DamagesPanel {p} {isOwner} {isCrew} currentUserId={$currentUser?.id} on:changed={load} />
    </div>

    <!-- 版本与确认历史 -->
    <div class="two-col">
      <div class="panel">
        <h2>方案版本（修改全程留痕）</h2>
        {#each [...p.versions].reverse() as v}
          <div class="version-card {v.id === p.current_version_id ? 'current' : ''}">
            <div class="row" style="justify-content:space-between">
              <strong>v{v.version_no} {v.id === p.current_version_id ? '· 当前版本' : ''}</strong>
              <span class="muted small">{fmtTime(v.created_at)} · {userName(v.created_by)}</span>
            </div>
            <div class="small muted">{v.plan.items.length} 件灯具{v.note ? ` · 备注：${v.note}` : ''}</div>
          </div>
        {/each}
      </div>
      <div class="panel">
        <h2>确认记录（每次修改后的重新确认结果）</h2>
        {#if p.confirmations.length}
          {#each [...p.confirmations].reverse() as c}
            <div class="version-card">
              <div class="row" style="justify-content:space-between">
                <span class="badge {c.decision === 'confirmed' ? 'completed' : c.decision === 'rejected' ? 'rejected' : 'changes_requested'}">
                  {c.decision === 'confirmed' ? '✅ 确认' : c.decision === 'rejected' ? '驳回' : '要求修改'}
                </span>
                <span class="muted small">{fmtTime(c.created_at)} · {userName(c.confirmed_by)}</span>
              </div>
              {#if c.power_summary}
                <div class="small muted" style="margin-top:4px">
                  总功率 {c.power_summary.total_power_w}W；
                  {#each c.power_summary.circuits as cc}{cc.circuit_name} {cc.total_after_w}/{cc.max_power_w}W；{/each}
                </div>
              {/if}
              {#if c.note}<div class="small">意见：{c.note}</div>{/if}
            </div>
          {/each}
        {:else}
          <p class="muted small">尚无确认记录</p>
        {/if}
      </div>
    </div>
  </div>
{/if}
