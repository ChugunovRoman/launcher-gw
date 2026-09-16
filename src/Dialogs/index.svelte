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
  import FactionMeta from "./FactionMeta.svelte";
  import FactionRename from "./FactionRename.svelte";
  import FactionDelete from "./FactionDelete.svelte";
  import FactionApplyConfirm from "./FactionApplyConfirm.svelte";
  import FactionGameRunning from "./FactionGameRunning.svelte";
  import FactionResult from "./FactionResult.svelte";
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
<FactionMeta />
<FactionRename />
<FactionDelete />
<FactionApplyConfirm />
<FactionGameRunning />
<FactionResult />

<style></style>
