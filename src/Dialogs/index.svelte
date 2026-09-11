<script lang="ts">
  import RestartApp from "./RestartApp.svelte";
  import RemoveVersion from "./RemoveVersion.svelte";
  import RemoveProfile from "./RemoveProfile.svelte";
  import CannotApplyProfile from "./CannotApplyProfile.svelte";
  import ApplyProfileOk from "./ApplyProfileOk.svelte";
  import AddVersion from "./AddVersion.svelte";
  import PatchNotes from "./PatchNotes.svelte";
  import MaxPerformancePresetWarning from "./MaxPerformancePresetWarning.svelte";
  import LaunchError from "./LaunchError.svelte";
  import TempPathWarning from "./TempPathWarning.svelte";
</script>

<RestartApp />
<RemoveVersion />
<RemoveProfile />
<CannotApplyProfile />
<ApplyProfileOk />
<AddVersion maxWidth="90%" />
<PatchNotes />
<MaxPerformancePresetWarning />
<LaunchError />
<TempPathWarning />

<style></style>
