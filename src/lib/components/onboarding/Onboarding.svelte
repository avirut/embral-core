<script lang="ts">
    import { onMount } from "svelte";
    import { Mic, Volume2, Check } from "lucide-svelte";
    import type { AutoStartPolicy } from "$lib/types";
    import { configStore } from "$lib/stores/config.svelte";
    import { modelsStore } from "$lib/stores/models.svelte";
    import { speakersStore } from "$lib/stores/speakers.svelte";
    import { Button } from "$lib/components/ui/button";
    import { Input } from "$lib/components/ui/input";
    import EmbralIcon from "$lib/components/EmbralIcon.svelte";
    import { formatBytes } from "$lib/utils/bytes";
    import { cn } from "$lib/utils";

    let step = $state(0);
    const stepCount = 5;

    let selectedModel = $state("zipformer-en");
    let policyChoice = $state<AutoStartPolicy>("prompt");
    let notesChoice = $state<"none" | "builtin">("none");
    let userName = $state("");

    onMount(() => {
        modelsStore.refresh();
    });

    let asrModels = $derived(
        modelsStore.statuses.filter((m) => m.kind === "streaming_asr"),
    );
    let selected = $derived(modelsStore.status(selectedModel));
    let punctModel = $derived(
        modelsStore.statuses.find((m) => m.kind === "punctuation"),
    );
    let downloading = $derived(
        modelsStore.isDownloading(selectedModel) ||
            (punctModel ? modelsStore.isDownloading(punctModel.id) : false),
    );


    async function downloadSelected() {
        // The Zipformers want the punctuation model alongside; NeMo-family
        // models punctuate natively.
        const jobs = [modelsStore.download(selectedModel)];
        if (selected && !selected.native_punctuation && punctModel && !punctModel.present) {
            jobs.push(modelsStore.download(punctModel.id));
        }
        await Promise.all(jobs);
    }

    let downloadPct = $derived.by(() => {
        const main = modelsStore.fraction(selectedModel);
        return main == null ? null : Math.round(main * 100);
    });

    async function finish() {
        const cfg = configStore.config;
        if (!cfg) return;
        // The name creates the "you" profile in the speaker registry — the
        // dominance prior and voice matching hang identity off it.
        const name = userName.trim();
        if (name) {
            await speakersStore.save({
                name,
                notes: "",
                is_you: true,
            });
        }
        await configStore.save({
            ...cfg,
            onboarding_completed: true,
            local_asr_model: selectedModel,
            // Local transcription is the default story; cloud providers can be
            // picked in Settings any time.
            transcription_provider: "local",
            auto_start_policy: policyChoice,
            // The engine stays at its default; this step is the on/off.
            summaries_enabled: notesChoice === "builtin",
        });
    }

    let builtinLlm = $derived(modelsStore.status("qwen3-4b"));
    let llmRuntime = $derived(modelsStore.status("llama-server"));
    let builtinReady = $derived(
        (builtinLlm?.present ?? false) && (llmRuntime?.present ?? false),
    );
    let builtinDownloading = $derived(
        modelsStore.isDownloading("qwen3-4b") || modelsStore.isDownloading("llama-server"),
    );
    let builtinPct = $derived.by(() => {
        const f = modelsStore.fraction("qwen3-4b");
        return f == null ? null : Math.round(f * 100);
    });

    function downloadBuiltin() {
        void Promise.all([
            modelsStore.download("llama-server"),
            modelsStore.download("qwen3-4b"),
        ]);
    }

    const policyOptions: { id: AutoStartPolicy; title: string; body: string }[] = [
        {
            id: "always",
            title: "Automatically",
            body: "embral watches for calls and starts recording on its own.",
        },
        {
            id: "prompt",
            title: "Ask first",
            body: "embral notices the call and asks if you want to record it.",
        },
        {
            id: "manual",
            title: "Never",
            body: "Nothing records until you press the record button.",
        },
    ];

    const notesOptions: {
        id: "none" | "builtin";
        title: string;
        body: string;
    }[] = [
        {
            id: "builtin",
            title: "On this computer — no setup needed",
            body: "A bundled model (Qwen3 4B, ~2.5 GB download) writes summaries fully privately.",
        },
        {
            id: "none",
            title: "Not for now",
            body: "Turn it on later in Settings. Meetings still get full transcripts.",
        },
    ];
</script>

<div class="fixed inset-0 z-50 flex items-center justify-center bg-background">
    <div class="flex w-full max-w-xl flex-col px-8">
        <div class="mb-8 flex items-center gap-2">
            <EmbralIcon size={20} />
            <span class="text-base font-semibold tracking-tight">embral</span>
        </div>

        {#if step === 0}
            <h1 class="font-display text-2xl tracking-tight">Welcome to embral</h1>
            <p class="mt-3 text-sm text-muted-foreground">
                To transcribe your meetings, embral listens to two things at once:
            </p>
            <div class="mt-6 space-y-4">
                <div class="flex items-start gap-3">
                    <Mic size={18} class="mt-0.5 shrink-0 text-primary" />
                    <div>
                        <p class="text-sm font-medium">Microphone</p>
                        <p class="text-xs text-muted-foreground">Your voice during calls.</p>
                    </div>
                </div>
                <div class="flex items-start gap-3">
                    <Volume2 size={18} class="mt-0.5 shrink-0 text-primary" />
                    <div>
                        <p class="text-sm font-medium">System audio</p>
                        <p class="text-xs text-muted-foreground">
                            Everyone else on the call — even with headphones in.
                        </p>
                    </div>
                </div>
            </div>
            <p class="mt-6 text-xs text-muted-foreground">
                With local transcription, audio never leaves your computer.
            </p>
        {:else if step === 1}
            <h1 class="font-display text-2xl tracking-tight">Pick a speech model</h1>
            <p class="mt-3 text-sm text-muted-foreground">
                Runs on this computer. You can switch models any time in Settings.
            </p>
            <div class="mt-6 space-y-2">
                {#each asrModels as m (m.id)}
                    <button
                        class={cn(
                            "w-full rounded-lg border p-3 text-left transition-colors hover:bg-accent/50",
                            selectedModel === m.id
                                ? "border-primary ring-1 ring-primary"
                                : "border-border",
                        )}
                        onclick={() => (selectedModel = m.id)}
                    >
                        <div class="flex items-center justify-between">
                            <span class="text-sm font-medium">{m.display_name}</span>
                            <span class="text-xs text-muted-foreground">
                                {m.present ? "Downloaded" : `~${formatBytes(m.total_bytes)}`}
                            </span>
                        </div>
                        <p class="mt-0.5 text-xs text-muted-foreground">{m.note}</p>
                    </button>
                {/each}
            </div>
            <div class="mt-4">
                {#if selected?.present}
                    <p class="flex items-center gap-1.5 text-sm text-primary">
                        <Check size={15} /> Ready to transcribe locally.
                    </p>
                {:else if downloading}
                    <div class="space-y-1.5">
                        <div class="h-1.5 w-full overflow-hidden rounded-full bg-muted">
                            <div
                                class="h-full bg-primary transition-all duration-300"
                                style="width: {downloadPct ?? 0}%"
                            ></div>
                        </div>
                        <p class="text-xs text-muted-foreground">
                            Downloading… {downloadPct ?? 0}%
                        </p>
                    </div>
                {:else}
                    <Button size="sm" onclick={downloadSelected}>Download model</Button>
                {/if}
            </div>
        {:else if step === 2}
            <h1 class="font-display text-2xl tracking-tight">
                When should embral start recording?
            </h1>
            <p class="mt-3 text-sm text-muted-foreground">
                Choose how hands-on you want to be. You can fine-tune this — and
                which apps count — in Settings.
            </p>
            <div class="mt-6 space-y-2">
                {#each policyOptions as option (option.id)}
                    <button
                        class={cn(
                            "w-full rounded-lg border p-3 text-left transition-colors hover:bg-accent/50",
                            policyChoice === option.id
                                ? "border-primary ring-1 ring-primary"
                                : "border-border",
                        )}
                        onclick={() => (policyChoice = option.id)}
                    >
                        <p class="text-sm font-medium">{option.title}</p>
                        <p class="mt-0.5 text-xs text-muted-foreground">{option.body}</p>
                    </button>
                {/each}
            </div>
        {:else if step === 3}
            <h1 class="font-display text-2xl tracking-tight">Summarize every meeting?</h1>
            <p class="mt-3 text-sm text-muted-foreground">
                embral can write structured notes when a meeting ends — key
                takeaways, next steps, and topic sections.
            </p>
            <div class="mt-6 space-y-2">
                {#each notesOptions as option (option.id)}
                    <button
                        class={cn(
                            "w-full rounded-lg border p-3 text-left transition-colors hover:bg-accent/50",
                            notesChoice === option.id
                                ? "border-primary ring-1 ring-primary"
                                : "border-border",
                        )}
                        onclick={() => (notesChoice = option.id)}
                    >
                        <p class="text-sm font-medium">{option.title}</p>
                        <p class="mt-0.5 text-xs text-muted-foreground">{option.body}</p>
                    </button>
                {/each}
            </div>
            {#if notesChoice === "builtin"}
                <div class="mt-4">
                    {#if builtinReady}
                        <p class="flex items-center gap-1.5 text-sm text-primary">
                            <Check size={15} /> Ready to summarize on this computer.
                        </p>
                    {:else if builtinDownloading}
                        <div class="space-y-1.5">
                            <div class="h-1.5 w-full overflow-hidden rounded-full bg-muted">
                                <div
                                    class="h-full bg-primary transition-all duration-300"
                                    style="width: {builtinPct ?? 0}%"
                                ></div>
                            </div>
                            <p class="text-xs text-muted-foreground">
                                Downloading… {builtinPct ?? 0}% — you can keep going,
                                it finishes in the background.
                            </p>
                        </div>
                    {:else}
                        <Button size="sm" onclick={downloadBuiltin}>
                            Download the model (~2.5 GB)
                        </Button>
                    {/if}
                </div>
            {/if}
        {:else}
            <h1 class="font-display text-2xl tracking-tight">What should we call you?</h1>
            <p class="mt-3 text-sm text-muted-foreground">
                Used in your transcripts and summaries. It stays on your computer.
            </p>
            <Input
                bind:value={userName}
                placeholder="Your name"
                class="mt-6 max-w-sm"
                onkeydown={(e) => {
                    if (e.key === "Enter") void finish();
                }}
            />
        {/if}

        <div class="mt-10 flex items-center justify-between">
            <div class="flex gap-1.5">
                {#each Array(stepCount) as _, i (i)}
                    <span
                        class={cn(
                            "h-1.5 w-1.5 rounded-full",
                            i === step ? "bg-primary" : "bg-muted",
                        )}
                    ></span>
                {/each}
            </div>
            <div class="flex items-center gap-2">
                {#if step > 0}
                    <Button variant="ghost" size="sm" onclick={() => step--}>Back</Button>
                {/if}
                <Button variant="ghost" size="sm" onclick={finish}>Skip setup</Button>
                {#if step < stepCount - 1}
                    <Button size="sm" onclick={() => step++}>Continue</Button>
                {:else}
                    <Button size="sm" onclick={finish}>Finish</Button>
                {/if}
            </div>
        </div>
    </div>
</div>
