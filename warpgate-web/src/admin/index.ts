import { mount } from 'svelte'
import '../theme'
import { t } from 'common/i18n'
import App from './App.svelte'

document.title = t('Security gateway admin · MOAVI Desk')

mount(App, {
    // biome-ignore lint/style/noNonNullAssertion: x
    target: document.getElementById('app')!,
})
