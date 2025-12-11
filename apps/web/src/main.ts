import "./app.css";
import { mount } from "svelte";
import App from "./App.svelte";

const target = document.getElementById("app");

if (target) {
  // Clear the loading placeholder before mounting
  target.innerHTML = "";
  mount(App, { target });
}
