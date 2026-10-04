<script>
  import { onMount } from 'svelte';
  import { createEventDispatcher } from 'svelte';
  import { api, fmtTime } from '../api.js';
  import { userName, toast } from '../stores.js';

  export let p;
  export let isOwner;
  export let isCrew;
  export let currentUserId;
  const emit = createEventDispatcher();

  let damages = [];
  let noteBy = {};

  async function load() {
    damages = await api(`/projects/${p.id}/damages`);
  }
  onMount(load);

  // 父组件刷新时重新拉取
  $: if (p) load();

  async function requestWaiver(d) {
    try {
      await api(`/damages/${d.id}/waiver/request`, { method: 'POST' });
      toast('豁免申请已发出：本人不能审批自己项目的豁免', 'ok');
      await load();
    } catch (e) { toast(e.message, 'err'); }
  }
  async function decide(d, approve) {
    try {
      await api(`/damages/${d.id}/waiver/decision`, {
        method: 'POST',
        body: JSON.stringify({ approve, note: noteBy[d.id] || null }),
      });
      toast(approve ? '豁免已批准' : '豁免已驳回', 'ok');
      noteBy[d.id] = '';
      await load();
      emit('changed');
    } catch (e) { toast(e.message, 'err'); }
  }

  async function repairFixture(d) {
    try {
      await api(`/fixtures/${d.fixture_id}/status`, { method: 'POST', body: JSON.stringify({ status: 'ok' }) });
      toast(`${d.fixture_name} 已修复归库，可再次排期`, 'ok');
      await load();
      emit('changed');
    } catch (e) { toast(e.message, 'err'); }
  }

  const WAIVER = {
    none: { text: '未申请', cls: 'draft' },
    requested: { text: '待审批', cls: 'pending' },
    approved: { text: '已批准豁免', cls: 'completed' },
    rejected: { text: '豁免驳回', cls: 'rejected' },
  };
</script>

{#if damages.length === 0}
  <p class="muted small">暂无损坏记录。归还时选择“损坏”即自动建档并关联本次项目。</p>
{:else}
  <table>
    <thead>
    <thead>
      <tr><th>#</th><th>灯具</th><th>损坏描述</th><th>上报</th><th>豁免状态</th><th>操作</th></tr>
    </thead>
    <tbody>
      {#each damages as d}
        <tr>
          <td>{d.id}</td>
          <td>{d.fixture_name}</td>
          <td style="max-width:260px">{d.description}</td>
          <td class="small muted">{userName(d.reported_by)}<br />{fmtTime(d.created_at)}</td>
          <td>
            <span class="badge {WAIVER[d.waiver_status].cls}">{WAIVER[d.waiver_status].text}</span>
            {#if d.waiver_approver}<div class="small muted">审批：{userName(d.waiver_approver)}</div>{/if}
            {#if d.waiver_note}<div class="small muted">意见：{d.waiver_note}</div>{/if}
          </td>
          <td>
            {#if d.waiver_status === 'none' && isOwner}
              <button class="btn sm ghost" on:click={() => requestWaiver(d)}>申请豁免</button>
            {:else if d.waiver_status === 'requested'}
              {#if isCrew && currentUserId !== p.photographer_id}
                <input
                  type="text"
                  placeholder="审批意见"
                  bind:value={noteBy[d.id]}
                  style="width:150px;margin-bottom:4px"
                /><br />
                <button class="btn sm ok" on:click={() => decide(d, true)}>批准</button>
                <button class="btn sm danger" on:click={() => decide(d, false)}>驳回</button>
              {:else if currentUserId === p.photographer_id}
                <span class="muted small">本人无权审批（系统硬规则）</span>
              {/if}
            {/if}
            {#if isCrew}
              <br /><button class="btn sm ghost" style="margin-top:4px" on:click={() => repairFixture(d)}>🔧 修复归库</button>
            {/if}
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
{/if}
