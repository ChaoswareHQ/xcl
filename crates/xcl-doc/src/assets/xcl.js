document.querySelectorAll('pre').forEach(pre => {
    const btn = document.createElement('button');
    btn.className = 'copy-btn';
    btn.textContent = 'copy';
    btn.onclick = () => {
        const text = pre.querySelector('code').innerText;
        navigator.clipboard.writeText(text);
        btn.textContent = 'copied';
        btn.classList.add('copied');
        setTimeout(() => {
            btn.textContent = 'copy';
            btn.classList.remove('copied');
        }, 1500);
    };
    pre.appendChild(btn);
});
