import { createApp } from "vue";
import App from "./App.vue";
import { createVuetify } from 'vuetify';
import "./assets/style.css";

// Vuetify core + 项目内 MDI 图标子集。组件按**单个组件目录**导入：
// barrel 形式 `from 'vuetify/components'` 虽然 JS 侧能被 tree-shake 干净，但 vuetify 的
// package.json 把 `*.css` 声明为 sideEffects，导致未使用组件的样式照样进包
// （实测 vuetify CSS 296.9KB → 164.9KB，gzip 38.0 → 20.2KB）。
import 'vuetify/styles/core';
import './assets/mdi-subset.css';
// 自托管 UI 字体（Space Grotesk / Rajdhani），不再从 Google Fonts CDN 加载：
// 离线可用、不向外部发请求，CSP 也就不用为 fonts.googleapis/gstatic 开口子。
import './assets/fonts.css';

import { VApp } from 'vuetify/components/VApp';
import { VBtn } from 'vuetify/components/VBtn';
import { VBtnToggle } from 'vuetify/components/VBtnToggle';
import { VCard, VCardActions, VCardText, VCardTitle } from 'vuetify/components/VCard';
import { VChip } from 'vuetify/components/VChip';
import { VCombobox } from 'vuetify/components/VCombobox';
import { VDataTable } from 'vuetify/components/VDataTable';
import { VDialog } from 'vuetify/components/VDialog';
import { VForm } from 'vuetify/components/VForm';
import { VIcon } from 'vuetify/components/VIcon';
import { VList, VListItem, VListItemTitle } from 'vuetify/components/VList';
import { VMain } from 'vuetify/components/VMain';
import { VMenu } from 'vuetify/components/VMenu';
import { VNavigationDrawer } from 'vuetify/components/VNavigationDrawer';
import { VOverlay } from 'vuetify/components/VOverlay';
import { VPagination } from 'vuetify/components/VPagination';
import { VProgressCircular } from 'vuetify/components/VProgressCircular';
import { VProgressLinear } from 'vuetify/components/VProgressLinear';
import { VSelect } from 'vuetify/components/VSelect';
// VSpacer 虽然语义上属于网格布局，但它的目录是 VGrid。
import { VSpacer } from 'vuetify/components/VGrid';
import { VSwitch } from 'vuetify/components/VSwitch';
import { VTab, VTabs } from 'vuetify/components/VTabs';
import { VTextField } from 'vuetify/components/VTextField';
import { VWindow, VWindowItem } from 'vuetify/components/VWindow';
import { Ripple } from 'vuetify/directives';

const components = {
  VApp,
  VBtn,
  VBtnToggle,
  VCard,
  VCardActions,
  VCardText,
  VCardTitle,
  VChip,
  VCombobox,
  VDataTable,
  VDialog,
  VForm,
  VIcon,
  VList,
  VListItem,
  VListItemTitle,
  VMain,
  VMenu,
  VNavigationDrawer,
  VOverlay,
  VPagination,
  VProgressCircular,
  VProgressLinear,
  VSelect,
  VSpacer,
  VSwitch,
  VTab,
  VTabs,
  VTextField,
  VWindow,
  VWindowItem,
};

const vuetify = createVuetify({
  components,
  directives: { Ripple },
  theme: {
    defaultTheme: 'arknights',
    themes: {
      arknights: {
        dark: true,
        colors: {
          background: '#101218',
          surface: '#161827',
          'surface-variant': '#1c1f32',
          primary: '#3b82f6',
          'primary-hover': '#60a5fa',
          secondary: '#8b8fa3',
          accent: '#3b82f6',
          error: '#ef4444',
          info: '#3b82f6',
          success: '#10b981',
          warning: '#f59e0b',
          'on-background': '#e2e4ea',
          'on-surface': '#e2e4ea',
          'on-surface-variant': '#e2e4ea',
          'on-primary': '#0f1116',
          'on-secondary': '#0f1116',
          'on-success': '#0f1116',
          'on-warning': '#0f1116',
          'on-error': '#0f1116',
          'on-info': '#0f1116',
        },
      },
      light: {
        dark: false,
        colors: {
          background: '#efece6',
          surface: '#faf8f5',
          'surface-variant': '#ffffff',
          primary: '#2563eb',
          'primary-hover': '#3b82f6',
          secondary: '#5c6075',
          accent: '#3b82f6',
          error: '#dc2626',
          info: '#3b82f6',
          success: '#16a34a',
          warning: '#d97706',
          'on-background': '#1a1b23',
          'on-surface': '#1a1b23',
          'on-surface-variant': '#1a1b23',
          'on-primary': '#ffffff',
          'on-secondary': '#ffffff',
          'on-success': '#ffffff',
          'on-warning': '#ffffff',
          'on-error': '#ffffff',
          'on-info': '#ffffff',
        },
      },
    },
  },
  defaults: {
    VCard: {
      elevation: 0,
    },
    VBtn: {
      rounded: 'sm',
    },
    VTextField: {
      variant: 'outlined',
      density: 'comfortable',
    },
    VSelect: {
      variant: 'outlined',
      density: 'comfortable',
    },
    VDialog: {
      rounded: 'md',
    },
    VChip: {
      rounded: 'sm',
    },
  },
});

const app = createApp(App);
app.use(vuetify);
app.mount("#app");
