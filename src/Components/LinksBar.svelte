<script lang="ts">
  import { open } from '@tauri-apps/plugin-shell';

  import { Discord, Boosty, Steam, Tg, Vk, Youtube } from "../Icons";
</script>

<div class="linksbar">
  <Vk size={46} onclick={() => open("https://vk.com/wargreat") } />
  <Discord size={40} onclick={() => open("https://discord.com/servers/s-t-a-l-k-e-r-global-war-454947294027055114") } />
  <Tg size={60} onclick={() => open("https://t.me/wargreat") } />
  <Steam size={40} onclick={() => open("https://steamcommunity.com/groups/wargreat") } />
  <Youtube size={40} onclick={() => open("https://www.youtube.com/channel/UCz0iDM0TfhtxerHenIXTRuA") } />
  <Boosty size={40} onclick={() => open("https://boosty.to/globalwar") } />
</div>

<style>
  .linksbar {
    margin-top: 20px;
    display: grid;
    grid-template-rows: 80px 80px 80px 80px 80px 80px 1fr;
    align-items: center;
    background-color: rgba(0, 0, 0, 0.0);
    justify-items: anchor-center;
  }
</style>
