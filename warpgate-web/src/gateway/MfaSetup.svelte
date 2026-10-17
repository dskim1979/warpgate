<script lang="ts">
    import { t } from 'common/i18n'
    import { faArrowRight } from '@fortawesome/free-solid-svg-icons'
    import { Button } from '@sveltestrap/sveltestrap'
    import CreateOtpModal from 'admin/CreateOtpModal.svelte'
    import { reloadServerInfo, serverInfo } from 'gateway/lib/store'
    import Fa from 'svelte-fa'
    import { push } from 'svelte-spa-router'
    import { api } from './lib/api'

    let creatingOtpCredential = $state(false)

    async function createOtp(secretKey: number[]) {
        await api.addMyOtp({
            newOtpCredential: {
                secretKey,
            },
        })
        await reloadServerInfo()
        push('/')
    }
</script>

<div class="page-summary-bar">
    <h1>{t('Multi-factor authentication setup')}</h1>
</div>

<p>
    {t('Before you can use Warpgate, you need to add a second authentication factor to protect your account.')}
</p>

<Button
    color="primary"
    class="d-flex align-items-center align-self-start gap-2"
    onclick={e => {
        creatingOtpCredential = true
        e.preventDefault()
    }}
>
    {t('Set up one-time password')}
    <Fa icon={faArrowRight} />
</Button>

{#if creatingOtpCredential && $serverInfo?.username}
    <CreateOtpModal
        bind:isOpen={creatingOtpCredential}
        username={$serverInfo.username}
        create={createOtp}
    />
{/if}
