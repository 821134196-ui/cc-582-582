<script>
  import { onMount } from 'svelte';
  import { createEventDispatcher } from 'svelte';
  import { api, fmtTime } from '../api.js';
  import { currentUser, STATUS_LABELS, toast } from '../stores.js';
  import ProjectForm from '../components/ProjectForm.svelte';

  const dispatch = createEventDispatcher();
  let projects = [];
  let creating = false;
  let filter = '';

  async function load() {
    projects = await api('/projects');
  }
  onMount(load);

  $: shown = filter ? projects.filter((p) => p.status === filter) : projects;
  $: canCreate = $currentUser && ($currentUser.role === 'photographer' || $currentUser.role === 'admin');
</script>

{#if creating}
  <ProjectForm
    on:done={() => {
      creating = false;
      load();
    }}
  />
{:else}
  <div class="panel">
    <div class="row" style="justify-content:space-between">
      <h2 style="margin:0">拍摄方案与项目</h2>
      <div class="row">
        <select bind:value={filter}>
          <option value="">全部状态</option>
          {#each Object.entries(STATUS_LABELS) as [k, label]}
            <option value={k}>{label}</option>
          {/each}
        </select>
        {#if canCreate}
          <button class="btn" on:click={() => (creating = true)}>＋ 新建拍摄方案</button>
        {/if}
      </div>
    </div>

    <table style="margin-top:10px">
      <thead>
        <tr>
          <th>项目</th><th>摄影师</th><th>场地</th><th>时段</th><th>状态</th><th></th>
        </tr>
      </thead>
      <tbody>
        {#each shown as p (p.id)}
          <tr>
            <td><a on:click={() => dispatch('open', p.id)}>{p.name}</a><div class="muted small">#{p.id}</div></td>
            <td>{p.photographer_name}</td>
            <td>{p.studio_name}</td>
            <td class="small">{fmtTime(p.start_at)} ~ {fmtTime(p.end_at)}</td>
            <td><span class="badge {p.status}">{STATUS_LABELS[p.status]}</span></td>
            <td><button class="btn ghost sm" on:click={() => dispatch('open', p.id)}>打开</button></td>
          </tr>
        {:else}
          <tr><td colspan="6" class="muted" style="text-align:center;padding:24px">暂无项目</td></tr>
        {/each}
      </tbody>
    </table>
  </div>
{/if}
