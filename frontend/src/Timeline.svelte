<script>
  import { onMount } from 'svelte';
  import { api, fmtTime } from './api.js';

  export let meta;
  export let openProject;

  let data = null;

  onMount(load);
  async function load() {
    data = await api('GET', '/timeline');
  }

  $: allSlots = data ? [...data.venue_slots, ...data.fixture_slots] : [];
  $: minT = allSlots.length ? Math.min(...allSlots.map((s) => Date.parse(s.start_at))) : Date.now() - 3600_000;
  $: maxT = allSlots.length ? Math.max(...allSlots.map((s) => Date.parse(s.end_at))) : Date.now() + 3600_000;
  $: span = Math.max(maxT - minT, 1);

  function pos(s) {
    return ((Date.parse(s.start_at) - minT) / span) * 100;
  }
  function width(s) {
    return ((Date.parse(s.end_at) - Date.parse(s.start_at)) / span) * 100;
  }

  const COLORS = ['#4f8cff', '#34c78a', '#a981ff', '#f5a623', '#ef5b6b', '#38c9c4', '#e879f9'];
  function color(pid) { return COLORS[pid % COLORS.length]; }

  function rows(slots, refs) {
    return refs.map((r) => ({ ref: r, slots: slots.filter((s) => s.ref_id === r.id) }));
  }
</script>

<div class="panel">
  <h2>场地与灯具占用时段</h2>
  <p class="muted small">横条表示已确认（或部分归还中）项目占用的时间区间。同色代表同一项目；同一灯具行出现重叠即说明出现互斥冲突（系统会在确认时拦截）。</p>
</div>

{#if data}
  <div class="panel">
    <h3>🏛 场地</h3>
    <div class="gantt">
      {#each rows(data.venue_slots, meta.venues) as row}
        <div class="g-row">
          <div class="g-label">{row.ref.name}</div>
          <div class="g-track">
            {#each row.slots as s}
              <a class="g-bar" style="left:{pos(s)}%;width:{width(s)}%;background:{color(s.project_id)}"
                 title={`${s.project_name}（${s.owner_name}）${fmtTime(s.start_at)} ~ ${fmtTime(s.end_at)}`}
                 on:click={() => openProject(s.project_id)}>
                {s.project_name}
              </a>
            {/each}
          </div>
        </div>
      {/each}
    </div>
  </div>

  <div class="panel">
    <h3>💡 灯具</h3>
    <div class="gantt">
      {#each rows(data.fixture_slots, meta.fixtures.filter(f => f.active)) as row}
        <div class="g-row">
          <div class="g-label">{row.ref.code}<div class="muted small">{row.ref.model}</div></div>
          <div class="g-track">
            {#each row.slots as s}
              <a class="g-bar" style="left:{pos(s)}%;width:{width(s)}%;background:{color(s.project_id)}"
                 title={`${s.ref_name} → ${s.project_name}（${s.owner_name}）${fmtTime(s.start_at)} ~ ${fmtTime(s.end_at)}`}
                 on:click={() => openProject(s.project_id)}>
                {s.project_name}
              </a>
            {/each}
            {#if row.slots.length === 0}<span class="muted small" style="padding-left:8px;">空闲</span>{/if}
          </div>
        </div>
      {/each}
    </div>
  </div>

  <div class="panel">
    <h3>项目图例</h3>
    <div class="row">
      {#each [...new Map(allSlots.map(s => [s.project_id, s])).values()] as s}
        <span class="legend"><i style="background:{color(s.project_id)}"></i>{s.project_name} · {s.owner_name}</span>
      {/each}
      {#if allSlots.length === 0}<span class="muted">暂无任何占用</span>{/if}
    </div>
  </div>
{/if}

<style>
  .gantt { display: flex; flex-direction: column; gap: 8px; }
  .g-row { display: flex; align-items: stretch; gap: 10px; }
  .g-label { width: 180px; flex-shrink: 0; font-size: 13px; align-self: center; text-align: right; }
  .g-track {
    position: relative; flex: 1; height: 34px;
    background: repeating-linear-gradient(90deg, #0d1322 0, #0d1322 1px, transparent 1px, transparent 10%);
    border: 1px solid var(--line); border-radius: 6px;
  }
  .g-bar {
    position: absolute; top: 3px; bottom: 3px;
    border-radius: 5px; color: #fff; font-size: 11px;
    display: flex; align-items: center; padding: 0 8px;
    white-space: nowrap; overflow: hidden; cursor: pointer;
    text-shadow: 0 1px 2px rgba(0,0,0,.4);
  }
  .g-bar:hover { outline: 2px solid #fff; }
  .legend { display: inline-flex; align-items: center; gap: 6px; font-size: 12px; color: var(--muted); }
  .legend i { width: 12px; height: 12px; border-radius: 3px; display: inline-block; }
</style>
