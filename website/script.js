const demo = document.querySelector('.demo');
const demoButtons = document.querySelectorAll('[data-demo-button]');

for (const button of demoButtons) {
  button.addEventListener('click', () => {
    const state = button.dataset.demoButton;
    demo.dataset.demoState = state;
    for (const choice of demoButtons) {
      const selected = choice === button;
      choice.classList.toggle('active', selected);
      choice.setAttribute('aria-pressed', String(selected));
    }
  });
}
