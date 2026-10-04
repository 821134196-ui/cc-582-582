<script>
  import { onMount, onDestroy } from 'svelte';
  import { api } from '../api.js';
  import { studios } from '../stores.js';

  let studioId = 1;
  let readings = [];
  let now = '';
  let timer;

  async function load() {
    const d = await api(`/power/live?studio_id=${studioId}`);
    readings = d.readings;
    now = d.now;
  }
  onMount(() => {
    load();
    timer = setInterval(load, 3000);
  });
  onDestroy(() => clearInterval(timer));
  $: if (studioId) load();
</script>

<div class="panel">
  <div class="row" style="justify-content:space-between">
    <h2 style="margin:0">回路实时功率 <span class="muted small">（本地模拟电表，3 秒刷新；读数含 ±3W 抖动）</span></h2>
    <select bind:value={studioId}>
      {#each $studios as s}
        <option value={s.id}>{s.name}</option>
      {/each}
    </select>
  </div>

  <div class="grid" style="grid-template-columns:repeat(auto-fill,minmax(330px,1fr));margin-top:14px">
    {#each readings as r}
      <div class="panel" style="padding:14px;border-color:{r.over ? 'var(--danger)' : 'var(--border)'}">
        <div class="row" style="justify-content:space-between">
          <strong>{r.circuit_name}</strong>
          {#if r.over}<span class="badge rejected">⚠ 实测超限</span>{:else}<span class="badge completed">正常</span>{/if}
        </div>
        <div style="font-size:26px;font-weight:700;margin:6px 0" class={r.over ? '' : ''} style:color={r.over ? 'var(--danger)' : 'var(--ok)'}>
          {r.measured_w}<span class="muted small"> / {r.max_power_w} W</span>
        </div>
        <div class="progress"><div class={r.over ? 'over' : ''} style="width:{Math.min(100, r.utilization * 100)}%"></div></div>
        <div class="row small muted" style="justify-content:space-between;margin-top:6px">
          <span>排期负载 {r.scheduled_w}W</span>
          <span>利用率 {(r.utilization * 100).toFixed(0)}%</span>
        </div>
      </div>
    {/each}
  </div>
  <p class="muted small" style="margin-top:12px">采样时间：{now ? new Date(now).toLocaleString() : '—'}</p>
  <p class="muted small">说明：确认排期时系统按灯具标称功率做硬校验（超限无法确认）；本页为拍摄现场的模拟读数接口，仅用于监视。</p>
</div>
