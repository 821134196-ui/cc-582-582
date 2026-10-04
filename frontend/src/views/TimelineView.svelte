<script>
  import { onMount, onDestroy } from 'svelte';
  import { api, fmtTime } from '../api.js';
  import { studios } from '../stores.js';

  let studioId = 1;
  let data = { slots: [], bookings: [] };
  const DAY = 86400000;

  async function load() {
    data = await api(`/timeline?studio_id=${studioId}`);
  }
  onMount(() => {
    load();
  });
  $: if (studioId) load();

  // 时间域：所有占用的最小/最大时间，按天取整；至少覆盖今天
  $: allTimes = [...data.slots.map((s) => [s.start_at, s.end_at]).flat(),
                 ...data.bookings.map((b) => [b.start_at, b.end_at]).flat()]
                  .map((t) => new Date(t).getTime());
  $: t0 = (() => {
    const t = allTimes.length ? Math.min(...allTimes, Date.now()) : Date.now() - DAY;
    const d = new Date(t); d.setHours(0, 0, 0, 0); return d.getTime();
  })();
  $: t1 = (() => {
    const t = allTimes.length ? Math.max(...allTimes, Date.now()) : Date.now() + DAY;
    const d = new Date(t + DAY); d.setHours(0, 0, 0, 0); return d.getTime();
  })();
  $: days = Math.max(1, Math.round((t1 - t0) / DAY));
  $: studio = $studios.find((s) => s.id === studioId);
  $: fixtureRows = (studio?.fixtures || []).filter((f) => f.status !== 'retired');

  function barStyle(start, end) {
    const s = new Date(start).getTime();
    const e = new Date(end).getTime();
    const left = ((s - t0) / (t1 - t0)) * 100;
    const width = Math.max(1.2, ((e - s) / (t1 - t0)) * 100);
    return `left:${left}%;width:${width}%`;
  }
  function bookingsOf(fid) {
    return data.bookings.filter((b) => b.fixture_id === fid);
  }
  $: slots = data.slots.filter((s) => s.studio_id === studioId && s.status === 'held');

  const PALETTE = ['#4f8cff', '#34c77b', '#f5a524', '#a974ff', '#f4564d', '#38bdf8', '#fb923c', '#e879f9', '#2dd4bf'];
  const colorOf = new Map();
  function projectColor(pid) {
    if (!colorOf.has(pid)) colorOf.set(pid, PALETTE[colorOf.size % PALETTE.length]);
    return colorOf.get(pid);
  }

  function openProject(pid) {
    window.dispatchEvent(new CustomEvent('open-project', { detail: pid }));
  }

  function dayLabel(i) {
    const d = new Date(t0 + i * DAY);
    return `${d.getMonth() + 1}/${d.getDate()}`;
  }
  let nowLeft = 0;
  function updateNow() {
    nowLeft = ((Date.now() - t0) / (t1 - t0)) * 100;
  }
  let tick;
  onMount(() => {
    updateNow();
    tick = setInterval(updateNow, 30000);
  });
  onDestroy(() => clearInterval(tick));
</script>

<div class="panel">
  <div class="row" style="justify-content:space-between">
    <h2 style="margin:0">设备与场地占用时间线</h2>
    <select bind:value={studioId}>
      {#each $studios as s}
        <option value={s.id}>{s.name}</option>
      {/each}
    </select>
  </div>
  <p class="muted small">每行一件灯具 / 场地；色块即被项目占用的时段，点击色块打开项目。同一件灯具的色块永不重叠。</p>

  <div class="gantt">
    <table>
      <thead>
        <tr>
          <th class="label">资源</th>
          {#each Array(days) as _, i}
            <th style="width:{100 / days}%"><span class="small muted" style="padding:0 8px">{dayLabel(i)}</span></th>
          {/each}
        </tr>
      </thead>
      <tbody>
        <tr>
          <td class="label"><strong>🏛 {studio?.name}（场地）</strong></td>
          <td colspan={days} style="position:relative">
            {#if nowLeft >= 0 && nowLeft <= 100}<div class="nowline" style="left:{nowLeft}%"></div>{/if}
            {#each slots as s}
              <div
                class="bar held"
                style="{barStyle(s.start_at, s.end_at)};background:{projectColor(s.project_id)}"
                title={`${s.project_name}\n${fmtTime(s.start_at)} ~ ${fmtTime(s.end_at)}`}
                on:click={() => openProject(s.project_id)}
              >
                {s.project_name}
              </div>
            {/each}
          </td>
        </tr>
        {#each fixtureRows as f}
          <tr>
            <td class="label">
              {f.name}
              <span class="muted small">{f.power_w}W</span>
              {#if f.status === 'repair'}<span class="badge repair">维修中</span>{/if}
            </td>
            <td colspan={days} style="position:relative">
              {#if nowLeft >= 0 && nowLeft <= 100}<div class="nowline" style="left:{nowLeft}%"></div>{/if}
              {#each bookingsOf(f.id) as b}
                <div
                  class="bar {b.status}"
                  style="{barStyle(b.start_at, b.end_at)};{b.status === 'held' ? `background:${projectColor(b.project_id)}` : ''}"
                  title={`${b.project_name}｜${b.circuit_name}\n${fmtTime(b.start_at)} ~ ${fmtTime(b.end_at)}（${b.status === 'held' ? '占用中' : b.status === 'returned' ? '已归还' : '已取消'}）`}
                  on:click={() => openProject(b.project_id)}
                >
                  {b.project_name} · {b.circuit_name}
                </div>
              {/each}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>

  <div class="row small muted" style="margin-top:10px;gap:18px">
    <span><span style="display:inline-block;width:12px;height:12px;background:#4f8cff;border-radius:3px"></span> 占用中</span>
    <span><span style="display:inline-block;width:12px;height:12px;background:#34c77b;border-radius:3px"></span> 已归还</span>
    <span><span style="display:inline-block;width:12px;height:12px;background:#5b6678;border-radius:3px"></span> 已取消（原占用时段）</span>
    <span><span style="display:inline-block;width:2px;height:12px;background:#f4564d"></span> 当前时刻</span>
  </div>
</div>
