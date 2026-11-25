import { commands } from "./bindings";

export function App() {
  return (
    <main>
      <h1>WinOptSuite</h1>
      <span data-has-ipc={typeof commands.systemFacts === "function" ? "yes" : "no"} />
    </main>
  );
}

export default App;
