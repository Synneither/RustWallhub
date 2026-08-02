<script setup lang="ts">
import { appState, settleConfirm } from "../stores/app";
</script>

<template>
  <v-dialog
    :model-value="appState.confirm.visible"
    max-width="440"
    persistent
    @update:model-value="settleConfirm(false)"
  >
    <v-card class="confirm-dialog">
      <v-card-title class="text-heading pt-5 px-5 pb-2">
        {{ appState.confirm.title }}
      </v-card-title>
      <v-card-text class="text-body px-5 pb-4" style="white-space: pre-line; color: var(--text-secondary)">
        {{ appState.confirm.text }}
      </v-card-text>
      <v-card-actions class="px-5 pb-5 pt-0">
        <v-spacer />
        <v-btn variant="text" @click="settleConfirm(false)">取消</v-btn>
        <v-btn
          :color="appState.confirm.danger ? 'error' : 'primary'"
          variant="flat"
          @click="settleConfirm(true)"
        >
          {{ appState.confirm.confirmText }}
        </v-btn>
      </v-card-actions>
    </v-card>
  </v-dialog>
</template>

<style scoped>
.confirm-dialog {
  border-radius: var(--radius-xl);
  background: var(--surface-elevated) !important;
  border: var(--border-card);
}
</style>
