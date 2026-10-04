<script>
  import { createEventDispatcher, onMount } from 'svelte';
  import { api, STATUS_LABEL, fmtTime, toLocalInput, fromLocalInput } from './api.js';

  export let meta;
  export let user;
  const dispatch = createEventDispatcher();

  let projects = [];
  let creating = false;
  let form = { name: '', venue_id: 1, start: '', end: '' };
  let error = '';

  onMount(load);
  async function load() {
    projects = await api('GET', '/projects');
  }

  async function createProject() {
    error = '';
    try {
      const start = fromLocalInput(form.start);
      const end = fromLocalInput(form.end);
      if (!form.name || !start || !end) { error = '请完整填写名称与时段'; return; }
      const r = await api('POST', '/projects', { name: form.name, venue_id: form.venue_id, start, end });
      creating = false;
      form = { name: '', venue_id: 1, start: '', end: '' };
      dispatch('open', r.id);
    } catch (e) {
      error = e.message;
    }
  }

  function defaultTimes() {
    const now = new Date();
    now.setMinutes(0, 0, 0);
    now.setHours(now.getHours() + 1);
    const later = new Date(now.getTime() + 3 * 3600_000);
    form.start = toLocalInput(now.toISOString());
    form.end = toLocalInput(later.toISOString());
  }
</script>

<div class="panel">
  <div class="row" style="justify-content:space-between;">
    <h2 style="margin:0;">拍摄项目</h2>
    {#if user.role === 'photographer'}
      {#if !creating}
        <button class="primary" on:click={() => { creating = true; defaultTimes(); }}>+ 新建拍摄方案</button>
      {:else}
        <button on:click={() => (creating = false)}>取消</button>
      {/if}
    {/if}
  </div>

  {#if creating}
    <div style="margin-top:12px;max-width:720px;">
      <div class="grid2">
        <label>项目名称
          <input style="width:100%;margin-top:4px;" bind:value={form.name} placeholder="例如：秋装目录硬照" />
        </label>
        <label>场地
          <select style="margin-top:4px;" bind:value={form.venue_id}>
            {#each meta.venues as v}<option value={v.id}>{v.name}（{v.width_m}m×{v.height_m}m）</option>{/each}
          </select>
        </label>
        <label>开始时间
          <input style="margin-top:4px;" type="datetime-local" bind:value={form.start} />
        </label>
        <label>结束时间
          <input style="margin-top:4px;" type="datetime-local" bind:value={form.end} />
        </label>
      </div>
      {#if error}<div class="err-box">{error}</div>{/if}
      <div style="margin-top:10px;"><button class="primary" on:click={createProject}>创建并开始布光设计</button></div>
    </div>
  {/if}
</div>

<div class="panel">
  <table>
    <thead>
      <tr>
        <th>项目</th><th>摄影师</th><th>场地</th><th>拍摄时段</th><th>方案版本</th><th>最近确认</th><th>状态</th><th></th>
      </tr>
    </thead>
    <tbody>
      {#each projects as p}
        <tr class="clickable" on:click={() => dispatch('open', p.id)}>
          <td><strong>{p.name}</strong></td>
          <td>{p.photographer_name}</td>
          <td>{p.venue_name ?? '—'}</td>
          <td class="small">{fmtTime(p.shoot_start)} ~ {fmtTime(p.shoot_end)}</td>
          <td>v{p.version ?? 0}</td>
          <td>
            {#if p.last_result}
              <span class="tag {p.last_result}-res">{STATUS_LABEL[p.last_result] ?? p.last_result}</span>
            {:else}<span class="muted">—</span>{/if}
          </td>
          <td><span class="tag {p.status}">{STATUS_LABEL[p.status]}</span></td>
          <td><a>查看 →</a></td>
        </tr>
      {/each}
      {#if projects.length === 0}
        <tr><td colspan="8" class="muted" style="text-align:center;padding:24px;">暂无项目</td></tr>
      {/if}
    </tbody>
  </table>
</div>
