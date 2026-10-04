<script>
  import { createEventDispatcher } from 'svelte';

  export let studio;           // { width_m, height_m }
  export let items = [];       // [{fixture_id, circuit_id, x, y, note}]
  export let fixtures = [];    // catalog
  export let editable = false;
  export let selected = -1;

  const dispatch = createEventDispatcher();
  const W = 1200;
  const H = 900;
  const PAD = 60;

  $: sx = (W - PAD * 2) / (studio?.width_m || 10);
  $: sy = (H - PAD * 2) / (studio?.height_m || 8);
  const toX = (mx) => PAD + mx * sx;
  const toY = (my) => PAD + my * sy;

  function fixtureName(id) {
    return fixtures.find((f) => f.id === id)?.name || `灯#${id}`;
  }
  function fixturePower(id) {
    return fixtures.find((f) => f.id === id)?.power_w || 0;
  }

  let dragging = null;
  let svg;

  function pointerToMeter(ev) {
    const rect = svg.getBoundingClientRect();
    const px = ((ev.clientX - rect.left) / rect.width) * W;
    const py = ((ev.clientY - rect.top) / rect.height) * H;
    let mx = (px - PAD) / sx;
    let my = (py - PAD) / sy;
    mx = Math.max(0, Math.min(studio.width_m, mx));
    my = Math.max(0, Math.min(studio.height_m, my));
    return { x: Math.round(mx * 100) / 100, y: Math.round(my * 100) / 100 };
  }

  function startDrag(ev, idx) {
    if (!editable) {
      dispatch('select', { index: idx });
      return;
    }
    ev.preventDefault();
    dragging = idx;
    dispatch('select', { index: idx });
    window.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', onUp);
  }
  function onMove(ev) {
    if (dragging == null) return;
    const p = pointerToMeter(ev);
    dispatch('move', { index: dragging, x: p.x, y: p.y });
  }
  function onUp() {
    dragging = null;
    window.removeEventListener('pointermove', onMove);
    window.removeEventListener('pointerup', onUp);
  }
  function onFloorClick(ev) {
    if (!editable || selected < 0) return;
    const p = pointerToMeter(ev);
    dispatch('move', { index: selected, x: p.x, y: p.y });
  }

  const COLORS = ['#4f8cff', '#34c77b', '#f5a524', '#a974ff', '#f4564d', '#38bdf8', '#fb923c', '#e879f9'];
  function circuitColor(cid) {
    const ids = [...new Set((studio && studio.circuits ? studio.circuits : []).map((c) => c.id))];
    const i = ids.indexOf(cid);
    return COLORS[(i < 0 ? cid : i) % COLORS.length];
  }
</script>

<svg
  class="floorplan"
  viewBox="0 0 {W} {H}"
  bind:this={svg}
  on:click={onFloorClick}
  role="img"
  aria-label="摄影棚平面图"
>
  <!-- 地面网格 -->
  <rect x={PAD} y={PAD} width={W - PAD * 2} height={H - PAD * 2} fill="#101827" stroke="#33405c" stroke-width="2" />
  {#each Array(Math.floor(studio?.width_m || 10)) as _, i}
    <line x1={toX(i + 1)} y1={PAD} x2={toX(i + 1)} y2={H - PAD} stroke="#1d2940" stroke-width="1" />
  {/each}
  {#each Array(Math.floor(studio?.height_m || 8)) as _, i}
    <line x1={PAD} y1={toY(i + 1)} x2={W - PAD} y2={toY(i + 1)} stroke="#1d2940" stroke-width="1" />
  {/each}

  <!-- 标注 -->
  <text x={W / 2} y={H - 18} fill="#5d6b88" font-size="20" text-anchor="middle">{studio?.name} · {studio?.width_m}m × {studio?.height_m}m（每格 1m）</text>
  <text x={20} y={H / 2} fill="#5d6b88" font-size="18" text-anchor="middle" transform="rotate(-90 20 {H / 2})">宽 {studio?.height_m}m</text>

  <!-- 灯位 -->
  {#each items as it, idx (it.fixture_id + ':' + idx)}
    {@const cx = toX(it.x ?? studio.width_m / 2)}
    {@const cy = toY(it.y ?? studio.height_m / 2)}
    <g
      class="light-marker {selected === idx ? 'selected' : ''}"
      on:pointerdown={(ev) => startDrag(ev, idx)}
    >
      <line x1={cx} y1={cy} x2={cx} y2={cy + 34} stroke={circuitColor(it.circuit_id)} stroke-width="2" stroke-dasharray="4 4" />
      <circle cx={cx} cy={cy} r="17" fill={circuitColor(it.circuit_id)} stroke="#0c111c" stroke-width="3" />
      <text x={cx} y={cy + 5} text-anchor="middle" font-size="15" font-weight="700">{idx + 1}</text>
      <text x={cx} y={cy - 26} text-anchor="middle" font-size="16" fill="#dbe4f5">{fixtureName(it.fixture_id)}</text>
      <text x={cx} y={cy + 56} text-anchor="middle" font-size="14" fill="#8b97ad">{fixturePower(it.fixture_id)}W · ({it.x ?? '?'},{it.y ?? '?'})m</text>
    </g>
  {/each}

  {#if items.length === 0}
    <text x={W / 2} y={H / 2} fill="#5d6b88" font-size="24" text-anchor="middle">
      {editable ? '从右侧器材清单添加灯具，标记会出现在这里，可拖拽定位' : '暂无灯位安排'}
    </text>
  {/if}
</svg>
