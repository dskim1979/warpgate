<script lang="ts">
    import { t } from 'common/i18n'
    import NavListItem from 'common/NavListItem.svelte'
    import { serverInfo } from 'gateway/lib/store'
</script>

<div class="page-summary-bar">
    {#if $serverInfo}
        <h1>{$serverInfo.username}</h1>
    {/if}
</div>

<NavListItem
    title={t('API tokens')}
    description={t('Manage your API tokens')}
    href="/profile/api-tokens"
/>

{#if $serverInfo}
    {#if $serverInfo.ownCredentialManagementAllowed}
        <NavListItem
            title={t('Credentials')}
            description={t('Manage your passwords and keys')}
            href="/profile/credentials"
        />
    {/if}
{/if}

{#if $serverInfo?.ticketSelfServiceEnabled}
    <NavListItem
        title={t('Ticket requests')}
        description={t('Request and manage self-service access tickets')}
        href="/ticket-requests"
    />
{/if}
