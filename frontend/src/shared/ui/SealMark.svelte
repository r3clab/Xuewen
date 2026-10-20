<script lang="ts">
  // The signature: a cinnabar seal stamping 學 (xué, "learning"). Used in
  // exactly two places — TopBar wordmark and Welcome. Decorative only.
  let { size = 24 }: { size?: number } = $props();
</script>

<span
  aria-hidden="true"
  style={`width:${size}px;height:${size}px;font-size:${Math.round(size * 0.62)}px`}
  class="inline-flex select-none items-center justify-center rounded-[22%] bg-seal font-serif font-semibold leading-none text-paper"
>學</span>
