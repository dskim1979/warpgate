// MOAVI: browser tab title for an open web session, e.g.
// "rocky-01 (SSH) · 보안 게이트웨이", so several tabs can be told apart.
//
// Warpgate target names made by MOAVI Desk are internal
// ("moavi-<pool>-<vm>-desk_<id>[-ssh|-vnc]"); the target's description
// carries the portal's desktop name when the engine sets it.

import { t } from 'common/i18n'
import { api } from './api'

const MAX_LENGTH = 60

/** A readable name for a target: its description, else the name without MOAVI's prefix and suffix. */
export function targetDisplayName(name: string, description?: string | null): string {
    const fromDescription = description?.split('\n')[0]?.trim()
    const display =
        fromDescription ||
        name
            .replace(/^moavi-/, '')
            .replace(/-desk_[0-9a-f]+(-(ssh|vnc|rdp))?$/i, '') ||
        name
    return display.length > MAX_LENGTH ? `${display.slice(0, MAX_LENGTH - 1)}…` : display
}

/** Looks up the target's description (visible to the user in the target list). */
async function targetDescription(name: string): Promise<string | undefined> {
    try {
        const targets = await api.getTargets({ search: name })
        return targets.find(x => x.name === name)?.description || undefined
    } catch {
        return undefined
    }
}

export async function sessionTabTitle(targetName: string, targetKind?: string): Promise<string> {
    const display = targetDisplayName(targetName, await targetDescription(targetName))
    const protocol = targetKind ? ` (${targetKind.toUpperCase()})` : ''
    return `${display}${protocol} · ${t('Security gateway')}`
}
