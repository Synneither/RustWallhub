import { createApp } from "vue";
import App from "./App.vue";
import { createVuetify } from 'vuetify';
import "./assets/style.css"

// 导入 Vuetify 样式
import 'vuetify/styles';
// 导入图标字体
import '@mdi/font/css/materialdesignicons.css';

// 导入 Vuetify 组件
import * as components from 'vuetify/components';
import * as directives from 'vuetify/directives';

// 创建 Vuetify 实例
const vuetify = createVuetify({
    components,
    directives,
});

// 创建 Vue 应用
createApp(App)
    .use(vuetify) // 使用 Vuetify 插件
    .mount("#app");
