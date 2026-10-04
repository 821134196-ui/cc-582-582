<script>
  import { createEventDispatcher, onMount } from 'svelte';
  import { studios, toast } from '../stores.js';
  import { api, toLocalInput, fromLocalInput } from '../api.js';
  import FloorPlan from './FloorPlan.svelte';

  export let project = null; // 传入则为修改模式
  const dispatch = createEventDispatcher();

  let name = project?.name || '';
  let studioId = project?.studio_id || $studios[0]?.id || 1;
  let start = project ? toLocalInput(project.start_at) : '';
  let end = project ? toLocalInput(project.end_at) : '';
  let items = [];
  let note = '';
  let selected = 0;
  let report = null;
  let checking = false;
  let saving = false;

  onMount(() => {
    if (project) {
      // 取当前版本方案
      api(`/projects/${project.id}`).then((p) => {
        const v = p.versions.find((x) => x.id === p.current_version_id) || p.versions[p.versions.length - 1];
        items = (v?.plan?.items || []).map((it) => ({ ...it }));
        note = v?.note || '';
        selected = items.length ? 0 : -1;
        scheduleCheck();
      });
    } else {
      // 默认明天 09:00-18:00
      const d = new Date();
      d.setDate(d.getDate() + 1);
      d.setHours(9, 0, 0, 0);
      const d2 = new Date(d);
      d2.setHours(18, 0, 0, 0);
      start = toLocalInput(d.toISOString());
      end = toLocalInput(d2.toISOString());
      selected = -1;
    }
  });

  $: studio = $studios.find((s) => s.id === studioId) || $studios[0];

  function addFixture(fid) {
    const f = studio.fixtures.find((x) => x.id === fid);
    if (items.some((it) => it.fixture_id === fid)) {
      toast('该灯具已在清单中：同一灯具不能重复添加', 'err');
      return;
    }
    const n = items.length;
    items = [
      ...items,
      {
        fixture_id: fid,
        circuit_id: f.default_circuit_id || studio.circuits[0].id,
        x: Math.min(studio.width_m - 1, 2 + (n % 4) * 1.5),
        y: Math.min(studio.height_m - 1, 2 + Math.floor(n / 4) * 1.5),
        note: '',
      },
    ];
    selected = items.length - 1;
    scheduleCheck();
  }
  function removeItem(idx) {
    items = items.filter((_, i) => i !== idx);
    selected = Math.max(-1, Math.min(selected, items.length - 1));
    scheduleCheck();
  }
  function updateItem(idx, patch) {
    items = items.map((it, i) => (i === idx ? { ...it, ...patch } : it));
    scheduleCheck();
  }
  function moveItem(e) {
    updateItem(e.detail.index, { x: e.detail.x, y: e.detail.y });
  }

  let timer;
  function scheduleCheck() {
    clearTimeout(timer);
    timer = setTimeout(check, 350);
  }

  async function check() {
    if (!start || !end || !studio) return;
    checking = true;
    try {
      report = await api('/validate', {
        method: 'POST',
        body: JSON.stringify({
          studio_id: studioId,
          start_at: fromLocalInput(start),
          end_at: fromLocalInput(end),
          exclude_project_id: project?.id,
          items,
        }),
      });
    } catch (e) {
      report = e.report || null;
    } finally {
      checking = false;
    }
  }

  async function save(submitAfter = false) {
    if (!name.trim()) return toast('请填写方案名称', 'err');
    if (!items.length) return toast('请至少添加一盏灯', 'err');
    saving = true;
    try {
      const payload = {
        name: name.trim(),
        studio_id: studioId,
        start_at: fromLocalInput(start),
        end_at: fromLocalInput(end),
        items,
        note: note || null,
      };
      if (project) {
        const r = await api(`/projects/${project.id}`, { method: 'PUT', body: JSON.stringify(payload) });
        toast(r.needs_reconfirm ? '方案已修改：原占用已释放，需场务重新确认' : '新版本已保存', 'ok');
      } else {
        const r = await api('/projects', { method: 'POST', body: JSON.stringify(payload) });
        toast('草稿已创建（版本 v1）', 'ok');
        if (submitAfter) {
          await api(`/projects/${r.id}/submit`, { method: 'POST' });
          toast('已提交场务确认', 'ok');
        }
      }
      dispatch('done');
    } catch (e) {
      toast(e.report ? '预检未通过，存在设备冲突或回路超限，无法保存/提交' : e.message, 'err');
    } finally {
      saving = false;
    }
  }

  function fixtureRow(fid) {
    return studio?.fixtures.find((f) => f.id === fid);
  }
  function totalW() {
    return items.reduce((s, it) => s + (fixtureRow(it.fixture_id)?.power_w || 0), 0);
  }
</script>

<div class="grid">
  <div class="panel">
    <h2>{project ? `修改方案 #${project.id}（保存即生成新版本）` : '新拍摄方案'}</h2>
    <div class="row">
      <input type="text" placeholder="方案名称，如：秋季服装大片 Day1" bind:value={name} style="width:280px" />
      <select bind:value={studioId} on:change={scheduleCheck}>
        {#each $studios as s}
          <option value={s.id}>{s.name}（{s.width_m}×{s.height_m}m）</option>
        {/each}
      </select>
      <label class="small muted">开始</label>
      <input type="datetime-local" bind:value={start} on:change={scheduleCheck} />
      <label class="small muted">结束</label>
      <input type="datetime-local" bind:value={end} on:change={scheduleCheck} />
    </div>
    <div class="row" style="margin-top:8px">
      <input type="text" placeholder="备注（给场务的说明）" bind:value={note} style="flex:1" />
    </div>
  </div>

  <div class="two-col">
    <div class="panel">
      <h2>灯位平面图</h2>
      {#if studio}
        <FloorPlan
          {studio}
          {items}
          fixtures={studio.fixtures}
          editable={true}
          {selected}
          on:move={moveItem}
          on:select={(e) => (selected = e.detail.index)}
        />
      {/if}
      <p class="muted small" style="margin-top:8px">
        拖拽灯号定位，或选中后点击地面移动；灯号颜色表示所属回路。
      </p>
    </div>

    <div class="grid">
      <div class="panel">
        <h2>器材清单 <span class="muted small">共 {items.length} 件 · {totalW()}W</span></h2>
        {#if items.length}
          <table>
            <thead>
              <tr><th>#</th><th>灯具</th><th>功率</th><th>回路</th><th></th></tr>
            </thead>
            <tbody>
              {#each items as it, idx}
                <tr style:background={selected === idx ? 'var(--panel-2)' : 'transparent'}>
                  <td>{idx + 1}</td>
                  <td>
                    <a on:click={() => (selected = idx)}>{fixtureRow(it.fixture_id)?.name}</a>
                    <div class="muted small">({it.x ?? '?'}, {it.y ?? '?'})m</div>
                  </td>
                  <td>{fixtureRow(it.fixture_id)?.power_w}W</td>
                  <td>
                    <select value={it.circuit_id} on:change={(e) => updateItem(idx, { circuit_id: Number(e.target.value) })}>
                      {#each studio.circuits as c}
                        <option value={c.id}>{c.name} · 上限{c.max_power_w}W</option>
                      {/each}
                    </select>
                  </td>
                  <td><button class="btn ghost sm" on:click={() => removeItem(idx)}>移除</button></td>
                </tr>
              {/each}
            </tbody>
          </table>
        {:else}
          <p class="muted">尚未添加灯具</p>
        {/if}

        <h3>从设备库添加（维修中的灯具不可选）</h3>
        <div class="pill-row">
          {#each studio.fixtures as f}
            <button
              class="btn sm {f.status === 'ok' ? 'ghost' : ''}"
              disabled={f.status !== 'ok' || items.some((it) => it.fixture_id === f.id)}
              title={f.status !== 'ok' ? '该灯具维修中' : `${f.model || ''} ${f.power_w}W`}
              on:click={() => addFixture(f.id)}
            >
              {f.name} · {f.power_w}W{#if f.status !== 'ok'}（维修中）{/if}
            </button>
          {/each}
        </div>
      </div>

      <div class="panel">
        <h2>占用与功率预检 {#if checking}<span class="muted small">计算中…</span>{/if}</h2>
        {#if report}
          {#if report.ok}
            <div class="ok-box">✅ 预检通过：设备无冲突，各回路功率均在上限内</div>
          {:else}
            <div class="error-list">
              <strong>无法排期（{report.errors.length} 项问题）：</strong>
              <ul>
                {#each report.errors as er}
                  <li>{er}</li>
                {/each}
              </ul>
              <p class="small muted">系统不提供手工放行：冲突不解除则场务无法确认。</p>
            </div>
          {/if}

          <h3>回路负载（既有占用 + 本方案）</h3>
          {#each report.circuit_loads as c}
            <div style="margin-bottom:10px">
              <div class="row" style="justify-content:space-between">
                <strong>{c.circuit_name}</strong>
                <span class="{c.overloaded ? 'badge rejected' : 'badge completed'}">
                  {c.total_after_w} / {c.max_power_w}W {c.overloaded ? '· 超限' : `· 余量 ${c.headroom_w}W`}
                </span>
              </div>
              <div class="progress"><div class={c.overloaded ? 'over' : ''} style="width:{Math.min(100, (c.total_after_w / c.max_power_w) * 100)}%"></div></div>
              <div class="small muted">
                既有 {c.used_power_w}W + 本方案 {c.planned_power_w}W；
                {c.fixtures.map((f) => f.fixture_name).join('、') || '—'}
              </div>
            </div>
          {/each}
          {#if report.fixture_conflicts.length}
            <h3>灯具冲突</h3>
            {#each report.fixture_conflicts as cf}
              <div class="small">🔴 {cf.resource_name} 被「{cf.project_name}」占用</div>
            {/each}
          {/if}
          {#if report.studio_conflict}
            <h3>场地冲突</h3>
            <div class="small">🔴 {report.studio_conflict.resource_name} 被「{report.studio_conflict.project_name}」占用</div>
          {/if}
        {:else}
          <p class="muted small">填写时间与灯具后自动校验</p>
        {/if}
      </div>
    </div>
  </div>

  <div class="row" style="justify-content:flex-end">
    <button class="btn ghost" on:click={() => dispatch('done')}>取消</button>
    <button class="btn ghost" disabled={saving} on:click={() => save(false)}>保存草稿/新版本</button>
    <button class="btn" disabled={saving || (report && !report.ok)} on:click={() => save(true)}>
      {project ? '保存并等待重新确认' : '保存并提交场务确认'}
    </button>
  </div>
</div>
