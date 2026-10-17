// MOAVI: a portal deep link (#/web-desktop/start/<id>, #/web-ssh/start/<id>)
// opened in a browser that is already signed in as someone else (e.g. the
// local `admin`) gets 403 when starting the session. Offer to sign in again
// through the portal's SSO provider and come back to the same link.
//
// `?sso=<provider>` on the deep link names that provider and switches the
// account automatically; without it the only configured provider is used and
// the user is asked first. A second 403 right after switching (or for a user
// who already came from that provider) is a real "no access", never a loop.

import { errorStatus } from 'common/errors'
import { navigateToExternalUrl, routeQueryParams } from 'common/helpers'
import { api, type SsoProviderDescription } from './api'

export type AccountSwitch =
    | { kind: 'switch'; provider: string; label: string; auto: boolean }
    | { kind: 'denied' }

const RELOGIN_KEY = 'moavi-relogin-at'
const RELOGIN_WINDOW_MS = 120_000

function recentlyRelogged(): boolean {
    try {
        const last = Number(sessionStorage.getItem(RELOGIN_KEY) ?? 0)
        return Date.now() - last < RELOGIN_WINDOW_MS
    } catch {
        return false
    }
}

/** Call at component init: route query params are only read then. */
export function requestedSsoProvider(): string | undefined {
    return routeQueryParams().get('sso') ?? undefined
}

/** What to offer for an error from creating a web session, or `null` if it is not a 403. */
export async function accountSwitchFor(
    err: unknown,
    requestedProvider: string | undefined,
): Promise<AccountSwitch | null> {
    if (errorStatus(err) !== 403) {
        return null
    }
    let providers: SsoProviderDescription[] = []
    try {
        providers = await api.getSsoProviders()
    } catch {
        return null
    }
    const provider = requestedProvider
        ? providers.find(p => p.name === requestedProvider)
        : providers.length === 1
          ? providers[0]
          : undefined
    if (!provider) {
        return null
    }
    let current: string | undefined
    try {
        current = (await api.getInfo()).ssoProvider ?? undefined
    } catch {
        current = undefined
    }
    if (current === provider.name || recentlyRelogged()) {
        return { kind: 'denied' }
    }
    return {
        kind: 'switch',
        provider: provider.name,
        label: provider.label || provider.name,
        auto: !!requestedProvider,
    }
}

/** Log out of the current account and sign in through `provider`, returning here. */
export async function reloginWith(provider: string): Promise<void> {
    try {
        sessionStorage.setItem(RELOGIN_KEY, String(Date.now()))
    } catch {
        // ignore
    }
    try {
        await api.logout()
    } catch {
        // already logged out is fine
    }
    const next = location.pathname + location.hash
    const { url } = await api.startSso({ name: provider, next })
    navigateToExternalUrl(url)
}
