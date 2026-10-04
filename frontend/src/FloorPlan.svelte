<script>
  export let venue;
  export let lights;
  export let fixtures;
  export let readonly = false;
  export let selected = -1;

  const W = 720;
  const H = 480;
  $: sx = W / venue.width_m;
  $: sy = H / venue.height_m;

  let dragging = null;

  function toMeters(evt) {
    const svg = evt.currentTarget.ownerSVGElement || evt.currentTarget;
    const rect = svg.getBoundingClientRect();
    const x = ((evt.clientX - rect.left) / rect.width) * venue.width_m;
    const y = ((evt.clientY - rect.top) / rect.height) * venue.height_m;
    return {
      x: Math.min(venue.width_m, Math.max(0, x)),
      y: Math.min(venue.height_m, Math.max(0, y)),
    };
  }

  function onDown(i, e) {
    if (readonly) return;
    e.stopPropagation();
    selected = i;
    dragging = i;
    window.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', onUp, { once: true });
  }
  function onMove(e) {
    if (dragging === null) return;
    const m = coordsFromEvent(e);
    if (!m) return;
    lights[dragging].pos_x = +m.x.toFixed(2);
    lights[dragging].pos_y = +m.y.toFixed(2);
    lights = lights;
  }
  function onUp() {
    dragging = null;
    window.removeEventListener('pointermove', onMove);
  }
  function coordsFromEvent(e) {
    const svg = document.getElementById('floor-svg');
    if (!svg) return null;
    const rect = svg.getBoundingClientRect();
    const x = ((e.clientX - rect.left) / rect.width) * venue.width_m;
    const y = ((e.clientY - rect.top) / rect.height) * venue.height_m;
    return {
      x: Math.min(venue.width_m, Math.max(0, x)),
      y: Math.min(venue.height_m, Math.max(0, y)),
    };
  }

  function fixtureName(fid) {
    const f = fixtures.find((x) => x.id === fid);
    return f ? f.code : '待定灯具';
  }
</script>

<svg id="floor-svg" viewBox="0 0 {W} {H}" style="width:100%;background:#0d1322;border:1px solid var(--line);border-radius:8px;user-select:none;">
  <!-- 网格，每 1m -->
  {#each Array(Math.ceil(venue.width_m) + 1) as _, xi}
    <line x1={xi * sx} y1="0" x2={xi * sx} y2={H} stroke="#1c2740" stroke-width="1" />
  {/each}
  {#each Array(Math.ceil(venue.height_m) + 1) as _, yi}
    <line x1="0" y1={yi * sy} x2={W} y2={yi * sy} stroke="#1c2740" stroke-width="1" />
  {/each}

  <!-- 边界与门 -->
  <rect x="2" y="2" width={W - 4} height={H - 4} fill="none" stroke="#3b4870" stroke-width="2" rx="6" />
  <rect x={W / 2 - 40} y="{H - 4}" width="80" height="6" fill="#34c78a" />
  <text x={W / 2} y={H - 12} fill="#34c78a" font-size="12" text-anchor="middle">入口</text>

  <!-- 灯位 -->
  {#each lights as l, i}
    <g transform="translate({l.pos_x * sx},{l.pos_y * sy})"
       style="cursor:{readonly ? 'default' : 'grab'}"
       on:pointerdown={(e) => onDown(i, e)}>
      <circle r="16" fill={selected === i ? '#4f8cff' : '#a981ff'} fill-opacity="0.25"
              stroke={selected === i ? '#4f8cff' : '#a981ff'} stroke-width="2" />
      <circle r="5" fill={selected === i ? '#4f8cff' : '#a981ff'} />
      <text y="28" text-anchor="middle" font-size="11" fill="#cfd9f2">
        {i + 1}. {fixtureName(l.fixture_id)}
      </text>
    </g>
  {/each}

  {#if lights.length === 0}
    <text x={W / 2} y={H / 2} fill="#5d6a8c" text-anchor="middle" font-size="15">
      {readonly ? '本版本没有灯位' : '点击下方「添加灯位」，拖动画布中的圆点布置灯光'}
    </text>
  {/if}
</svg>
