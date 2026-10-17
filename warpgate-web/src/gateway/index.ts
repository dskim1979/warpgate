import { mount } from 'svelte'
import '../theme'
import { t } from 'common/i18n'
import Root from './Root.svelte'

document.title = t('Warpgate')

mount(Root, {
    // biome-ignore lint/style/noNonNullAssertion: x
    target: document.getElementById('app')!,
})
