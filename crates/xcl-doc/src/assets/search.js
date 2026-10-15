(function() {
    const input = document.getElementById('search');
    const results = document.getElementById('search-results');
    if (!input || !results) return;

    const index = window.SEARCH_INDEX || [];

    function search(query) {
        const q = query.toLowerCase().trim();
        if (!q) return [];

        // Split on '.' so "io.re" matches "io.read" with prefix on both parts
        const parts = q.split('.');

        return index.filter(item => {
            const name = item.name.toLowerCase();
            if (parts.length === 1) {
                return name.startsWith(parts[0]) || name.includes('.' + parts[0]);
            }
            // Multi-part query — match each part at segment boundaries
            const segments = name.split('.');
            return parts.every((p, i) => segments[i] && segments[i].startsWith(p));
        }).slice(0, 20);
    }

    function render(items) {
        if (items.length === 0) {
            results.classList.remove('visible');
            return;
        }
        results.innerHTML = items.map(item => `
            <a href="${item.path}">
                <span class="kind">${item.kind}</span>
                <code>${item.name}</code>
            </a>
        `).join('');
        results.classList.add('visible');
    }

    let timer;
    input.addEventListener('input', () => {
        clearTimeout(timer);
        timer = setTimeout(() => {
            render(search(input.value));
        }, 40);
    });

    input.addEventListener('keydown', (e) => {
        if (e.key === 'Escape') {
            input.value = '';
            render([]);
        }
        if (e.key === 'ArrowDown' && results.classList.contains('visible')) {
            const first = results.querySelector('a');
            if (first) { first.focus(); }
            e.preventDefault();
        }
    });

    document.addEventListener('click', (e) => {
        if (!input.contains(e.target) && !results.contains(e.target)) {
            results.classList.remove('visible');
        }
    });

    // Keyboard shortcut: `/` to focus search
    document.addEventListener('keydown', (e) => {
        if (e.key === '/' && document.activeElement !== input) {
            e.preventDefault();
            input.focus();
        }
    });
})();
