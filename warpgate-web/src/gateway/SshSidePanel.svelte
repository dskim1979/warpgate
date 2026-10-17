<script lang="ts">
    // MOAVI: right-side panel of the web terminal: session info and a file
    // browser of the target (SFTP over the session's own connection).
    import { t } from 'common/i18n'
    import {
        faArrowUp,
        faDownload,
        faFile,
        faFolder,
        faFolderPlus,
        faRotateRight,
        faTrash,
        faUpload,
    } from '@fortawesome/free-solid-svg-icons'
    import { onMount } from 'svelte'
    import Fa from 'svelte-fa'

    interface Props {
        sessionId: string
        connected: boolean
    }

    const { sessionId, connected }: Props = $props()

    interface SessionInfo {
        target_name: string
        username: string
        connected_at: number
        recording: boolean
        file_transfer: boolean
        max_upload_bytes: number
    }

    interface Entry {
        name: string
        is_dir: boolean
        is_link: boolean
        size?: number | null
        mtime?: number | null
    }

    interface Upload {
        id: number
        name: string
        loaded: number
        total: number
        state: 'queued' | 'uploading' | 'done' | 'error'
        message?: string
    }

    const base = $derived(`/@warpgate/api/web-ssh/sessions/${sessionId}`)

    let tab: 'info' | 'files' = $state('files')
    let info: SessionInfo | null = $state(null)
    let path = $state('')
    let entries: Entry[] = $state([])
    let truncated = $state(false)
    let loading = $state(false)
    let listError: string | null = $state(null)
    let confirmDelete: string | null = $state(null)
    let creatingFolder = $state(false)
    let newFolderName = $state('')
    let uploads: Upload[] = $state([])
    let dragging = $state(false)
    let fileInput: HTMLInputElement | undefined = $state()
    let uploadCounter = 0
    let uploadQueue: Promise<void> = Promise.resolve()

    async function messageFor(response: Response): Promise<string> {
        switch (response.status) {
            case 403:
                return t('Permission denied.')
            case 404:
                return t('File or folder not found.')
            case 409:
                return t('The request could not be completed.')
            case 413:
                return t('The file is too large.')
            case 502:
                return t('Could not reach the server for file transfer.')
        }
        const text = (await response.text()).trim()
        return text || t('Something went wrong.')
    }

    async function loadInfo() {
        try {
            const response = await fetch(`${base}/info`)
            if (response.ok) {
                info = await response.json()
            }
        } catch {
            // ignore; the panel shows what it has
        }
    }

    async function list(target?: string) {
        loading = true
        listError = null
        confirmDelete = null
        try {
            const query = target ? `?path=${encodeURIComponent(target)}` : ''
            const response = await fetch(`${base}/files${query}`)
            if (!response.ok) {
                listError = await messageFor(response)
                return
            }
            const data = await response.json()
            path = data.path
            entries = data.entries
            truncated = data.truncated
        } catch {
            listError = t('Could not reach the server for file transfer.')
        } finally {
            loading = false
        }
    }

    function join(dir: string, name: string): string {
        return dir.endsWith('/') ? `${dir}${name}` : `${dir}/${name}`
    }

    function parent(dir: string): string {
        if (dir === '/' || !dir.includes('/')) {
            return '/'
        }
        const up = dir.replace(/\/+$/, '').replace(/\/[^/]*$/, '')
        return up || '/'
    }

    function formatSize(size?: number | null): string {
        if (size === undefined || size === null) {
            return ''
        }
        const units = ['B', 'KB', 'MB', 'GB', 'TB']
        let value = size
        let unit = 0
        while (value >= 1024 && unit < units.length - 1) {
            value /= 1024
            unit++
        }
        return `${value.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`
    }

    function formatTime(seconds?: number | null): string {
        if (!seconds) {
            return ''
        }
        return new Date(seconds * 1000).toLocaleString()
    }

    async function remove(entry: Entry) {
        const target = join(path, entry.name)
        const query = `?path=${encodeURIComponent(target)}${entry.is_dir ? '&dir=true' : ''}`
        const response = await fetch(`${base}/files/delete${query}`, { method: 'POST' })
        confirmDelete = null
        if (!response.ok) {
            listError =
                entry.is_dir && response.status === 409
                    ? t('Only empty folders can be deleted.')
                    : await messageFor(response)
            return
        }
        await list(path)
    }

    async function createFolder() {
        const name = newFolderName.trim()
        if (!name) {
            return
        }
        const query = `?dir=${encodeURIComponent(path)}&name=${encodeURIComponent(name)}`
        const response = await fetch(`${base}/files/mkdir${query}`, { method: 'POST' })
        if (!response.ok) {
            listError = await messageFor(response)
            return
        }
        creatingFolder = false
        newFolderName = ''
        await list(path)
    }

    function uploadOne(upload: Upload, file: File, dir: string): Promise<void> {
        return new Promise(resolve => {
            const xhr = new XMLHttpRequest()
            const query = `?dir=${encodeURIComponent(dir)}&name=${encodeURIComponent(file.name)}`
            xhr.open('POST', `${base}/files/upload${query}`)
            xhr.setRequestHeader('Content-Type', 'application/octet-stream')
            upload.state = 'uploading'
            xhr.upload.onprogress = e => {
                upload.loaded = e.loaded
            }
            xhr.onload = () => {
                if (xhr.status >= 200 && xhr.status < 300) {
                    upload.state = 'done'
                    upload.loaded = upload.total
                } else {
                    upload.state = 'error'
                    upload.message =
                        xhr.status === 413
                            ? t('The file is too large.')
                            : xhr.status === 403
                              ? t('Permission denied.')
                              : xhr.responseText || t('Upload failed.')
                }
                resolve()
            }
            xhr.onerror = () => {
                upload.state = 'error'
                upload.message = t('Upload failed.')
                resolve()
            }
            xhr.send(file)
        })
    }

    /** Uploads into the folder shown now; files go one after another. */
    export function uploadFiles(files: FileList | File[]) {
        tab = 'files'
        const dir = path
        if (!dir || !info?.file_transfer) {
            return
        }
        for (const file of Array.from(files)) {
            uploads = [
                ...uploads,
                {
                    id: ++uploadCounter,
                    name: file.name,
                    loaded: 0,
                    total: file.size,
                    state: 'queued',
                },
            ]
            // the reactive proxy, so progress updates re-render
            const upload = uploads[uploads.length - 1]
            if (!upload) {
                continue
            }
            if (info && file.size > info.max_upload_bytes) {
                upload.state = 'error'
                upload.message = t('The file is too large.')
                continue
            }
            uploadQueue = uploadQueue.then(async () => {
                await uploadOne(upload, file, dir)
                if (dir === path) {
                    await list(path)
                }
            })
        }
    }

    function clearFinished() {
        uploads = uploads.filter(u => u.state === 'queued' || u.state === 'uploading')
    }

    function onDrop(e: DragEvent) {
        e.preventDefault()
        dragging = false
        if (e.dataTransfer?.files.length) {
            uploadFiles(e.dataTransfer.files)
        }
    }

    let started = false
    $effect(() => {
        if (connected && !started) {
            started = true
            loadInfo().then(() => {
                if (info?.file_transfer) {
                    list()
                }
            })
        }
    })

    onMount(() => {
        loadInfo()
    })
</script>

<aside
    class="side-panel d-flex flex-column"
    class:dragging
    ondragover={e => {
        e.preventDefault()
        dragging = true
    }}
    ondragleave={() => (dragging = false)}
    ondrop={onDrop}
>
    <div class="tabs d-flex">
        <button
            type="button"
            class="btn btn-sm flex-grow-1"
            class:active={tab === 'info'}
            onclick={() => (tab = 'info')}
        >{t('Info')}</button>
        <button
            type="button"
            class="btn btn-sm flex-grow-1"
            class:active={tab === 'files'}
            onclick={() => (tab = 'files')}
        >{t('Files')}</button>
    </div>

    {#if tab === 'info'}
        <dl class="info p-3 small">
            <dt>{t('Target')}</dt>
            <dd>{info?.target_name ?? ''}</dd>
            <dt>{t('User')}</dt>
            <dd>{info?.username ?? ''}</dd>
            <dt>{t('Connected since')}</dt>
            <dd>{formatTime(info?.connected_at)}</dd>
            <dt>{t('Recording')}</dt>
            <dd>{info ? (info.recording ? t('On') : t('Off')) : ''}</dd>
            <dt>{t('File transfer')}</dt>
            <dd>{info ? (info.file_transfer ? t('On') : t('Off')) : ''}</dd>
        </dl>
    {:else if info && !info.file_transfer}
        <div class="p-3 small text-muted">{t('File transfer is turned off for this server.')}</div>
    {:else if !connected && !path}
        <div class="p-3 small text-muted">{t('Files appear once connected.')}</div>
    {:else}
        <div class="toolbar d-flex align-items-center gap-1 px-2 py-1">
            <button
                type="button"
                class="btn btn-sm btn-link"
                title={t('Up')}
                disabled={!path || path === '/' || loading}
                onclick={() => list(parent(path))}
            ><Fa icon={faArrowUp} fw /></button>
            <button
                type="button"
                class="btn btn-sm btn-link"
                title={t('Refresh')}
                disabled={loading}
                onclick={() => list(path || undefined)}
            ><Fa icon={faRotateRight} fw /></button>
            <button
                type="button"
                class="btn btn-sm btn-link"
                title={t('New folder')}
                disabled={!path}
                onclick={() => {
                    creatingFolder = !creatingFolder
                    newFolderName = ''
                }}
            ><Fa icon={faFolderPlus} fw /></button>
            <button
                type="button"
                class="btn btn-sm btn-primary ms-auto d-flex align-items-center gap-1"
                disabled={!path}
                onclick={() => fileInput?.click()}
            ><Fa icon={faUpload} fw />{t('Upload')}</button>
            <input
                bind:this={fileInput}
                type="file"
                multiple
                hidden
                onchange={e => {
                    const input = e.currentTarget as HTMLInputElement
                    if (input.files?.length) {
                        uploadFiles(input.files)
                    }
                    input.value = ''
                }}
            >
        </div>
        <div class="path px-2 small text-muted text-truncate" title={path}>{path}</div>

        {#if creatingFolder}
            <form
                class="d-flex gap-1 px-2 py-1"
                onsubmit={e => {
                    e.preventDefault()
                    createFolder()
                }}
            >
                <!-- svelte-ignore a11y_autofocus -->
                <input
                    class="form-control form-control-sm"
                    placeholder={t('Folder name')}
                    bind:value={newFolderName}
                    autofocus
                >
                <button type="submit" class="btn btn-sm btn-primary">{t('Create')}</button>
            </form>
        {/if}

        {#if listError}
            <div class="alert alert-warning small m-2 py-1 px-2">{listError}</div>
        {/if}

        <div class="entries flex-grow-1">
            {#each entries as entry (entry.name)}
                <div class="entry d-flex align-items-center gap-2 px-2">
                    {#if entry.is_dir}
                        <button
                            type="button"
                            class="btn btn-link btn-sm name text-start text-truncate p-0"
                            onclick={() => list(join(path, entry.name))}
                        ><Fa icon={faFolder} fw class="me-1" />{entry.name}</button>
                    {:else}
                        <a
                            class="name text-truncate small"
                            href={`${base}/files/download?path=${encodeURIComponent(join(path, entry.name))}`}
                            download={entry.name}
                            title={t('Download')}
                        ><Fa icon={faFile} fw class="me-1" />{entry.name}</a>
                        <span class="size small text-muted">{formatSize(entry.size)}</span>
                    {/if}
                    {#if confirmDelete === entry.name}
                        <button
                            type="button"
                            class="btn btn-sm btn-danger py-0"
                            onclick={() => remove(entry)}
                        >{t('Delete')}</button>
                        <button
                            type="button"
                            class="btn btn-sm btn-secondary py-0"
                            onclick={() => (confirmDelete = null)}
                        >{t('Cancel')}</button>
                    {:else}
                        {#if !entry.is_dir}
                            <a
                                class="btn btn-sm btn-link p-0"
                                href={`${base}/files/download?path=${encodeURIComponent(join(path, entry.name))}`}
                                download={entry.name}
                                title={t('Download')}
                            ><Fa icon={faDownload} fw /></a>
                        {/if}
                        <button
                            type="button"
                            class="btn btn-sm btn-link p-0 text-danger"
                            title={t('Delete')}
                            onclick={() => (confirmDelete = entry.name)}
                        ><Fa icon={faTrash} fw /></button>
                    {/if}
                </div>
            {:else}
                {#if !loading && !listError}
                    <div class="p-3 small text-muted">{t('This folder is empty.')}</div>
                {/if}
            {/each}
            {#if truncated}
                <div class="p-2 small text-muted">{t('Only the first 5000 items are shown.')}</div>
            {/if}
        </div>

        {#if uploads.length}
            <div class="uploads p-2 small">
                {#each uploads as upload (upload.id)}
                    <div class="upload mb-1">
                        <div class="d-flex">
                            <span class="text-truncate me-auto">{upload.name}</span>
                            <span class="ms-2 text-nowrap">
                                {#if upload.state === 'done'}
                                    {t('Done')}
                                {:else if upload.state === 'error'}
                                    {t('Failed')}
                                {:else if upload.state === 'queued'}
                                    {t('Waiting')}
                                {:else}
                                    {Math.floor((upload.loaded / Math.max(upload.total, 1)) * 100)}%
                                {/if}
                            </span>
                        </div>
                        {#if upload.state === 'error'}
                            <div class="text-danger">{upload.message}</div>
                        {:else}
                            <div class="progress" style="height: 4px">
                                <div
                                    class="progress-bar"
                                    style={`width: ${Math.floor((upload.loaded / Math.max(upload.total, 1)) * 100)}%`}
                                ></div>
                            </div>
                        {/if}
                    </div>
                {/each}
                <button type="button" class="btn btn-sm btn-link p-0" onclick={clearFinished}>
                    {t('Clear finished')}
                </button>
            </div>
        {/if}

        {#if info}
            <div class="hint px-2 pb-2 small text-muted">
                {t('Drop files here or on the terminal. Up to {size} per file.', {
                    size: formatSize(info.max_upload_bytes),
                })}
            </div>
        {/if}
    {/if}
</aside>

<style lang="scss">
    .side-panel {
        width: 340px;
        flex-shrink: 0;
        background: #1b1d22;
        color: #d6d9e0;
        border-left: 1px solid #2c2f36;
        overflow: hidden;

        &.dragging {
            outline: 2px dashed #1668e3;
            outline-offset: -4px;
        }
    }

    .tabs .btn {
        color: #aab0bc;
        border-radius: 0;
        border-bottom: 2px solid transparent;

        &.active {
            color: #fff;
            border-bottom-color: #1668e3;
        }
    }

    .info {
        dt {
            color: #8a909c;
            font-weight: normal;
        }

        dd {
            margin-bottom: 0.6rem;
            word-break: break-all;
        }
    }

    .entries {
        overflow-y: auto;
    }

    .entry {
        min-height: 30px;

        &:hover {
            background: #262930;
        }

        .name {
            flex: 1 1 auto;
            min-width: 0;
            color: #d6d9e0;
            text-decoration: none;
        }

        .size {
            flex-shrink: 0;
        }
    }

    .uploads {
        border-top: 1px solid #2c2f36;
        max-height: 30%;
        overflow-y: auto;
    }
</style>
