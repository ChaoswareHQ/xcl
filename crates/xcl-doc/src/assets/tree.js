// ============================================================
// XCL Standard Library — Interactive Tree Visualization
// D3.js collapsible tree showing all modules, types, and functions
// ============================================================

(function() {
    "use strict";

    // The tree data is injected by the Rust renderer as a <script id="tree-data"> block
    var treeData = null;

    function init() {
        var dataEl = document.getElementById("tree-data");
        if (!dataEl) return;
        try {
            treeData = JSON.parse(dataEl.textContent);
        } catch(e) {
            console.error("Failed to parse tree data:", e);
            return;
        }
        renderTree();
    }

    function renderTree() {
        var container = document.getElementById("tree-container");
        if (!container) return;

        // Detect dark mode
        var isDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
        var nodeFill = isDark ? "#1c1c1c" : "#ffffff";
        var nodeStroke = isDark ? "#7ba4f7" : "#5b8af5";
        var linkColor = isDark ? "#555555" : "#e8e8e8";
        var textColor = isDark ? "#e0e0e0" : "#2c2c2c";
        var textMuted = isDark ? "#a8a8a8" : "#4a4a4a";
        var badgeBg = isDark ? "rgba(123,164,247,0.15)" : "rgba(91,138,245,0.1)";
        var badgeFg = isDark ? "#7ba4f7" : "#5b8af5";

        var width = container.clientWidth || 900;
        var margin = {top: 20, right: 20, bottom: 20, left: 20};
        var height = Math.max(800, treeData.children ? countNodes(treeData) * 18 + 100 : 800);

        var svg = d3.select(container)
            .append("svg")
            .attr("width", "100%")
            .attr("height", height)
            .attr("viewBox", [0, 0, width, height])
            .attr("style", "max-width: 100%; height: auto;");

        var g = svg.append("g")
            .attr("transform", "translate(" + margin.left + "," + margin.top + ")");

        var duration = 500;
        var root = d3.hierarchy(treeData);
        root.x0 = height / 2;
        root.y0 = 0;

        // Initially collapse after level 2
        root.children.forEach(collapse);

        g.attr("transform", "translate(80,0)");

        d3.select(self.frameElement).style("height", height + "px");

        update(root);

        function collapse(d) {
            if (d.children) {
                d._children = d.children;
                d._children.forEach(collapse);
                d.children = null;
            }
        }

        function update(source) {

            // Assigns the x and y position for the nodes
            var treeLayout = d3.tree()
                .nodeSize([18, width - margin.left - margin.right])
                .separation(function(a, b) { return a.parent == b.parent ? 1 : 2; });

            var nodes = treeLayout(root).descendants();

            // Normalize for fixed-depth
            nodes.forEach(function(d) { d.y = d.depth * 220; });

            // ****************** Nodes section ******************

            var node = g.selectAll('g.node')
                .data(nodes, function(d) { return d.id || (d.id = ++nodeId); });

            // Enter any new modes at the parent's previous position.
            var nodeEnter = node.enter().append('g')
                .attr('class', 'node')
                .attr("transform", function(d) {
                    return "translate(" + source.y0 + "," + source.x0 + ")";
                })
                .on('click', function(d) {
                    if (d.childrenClick) {
                        d.childrenClick(d);
                    } else {
                        click(d);
                        update(d);
                    }
                });

            nodeEnter.append('circle')
                .attr('class', 'node-circle')
                .attr('r', 1e-6)
                .style("fill", function(d) {
                    return d._children ? nodeStroke : (d.children ? nodeStroke : nodeFill);
                })
                .style("stroke", nodeStroke)
                .style("cursor", function(d) {
                    return d.children || d._children ? "pointer" : "default";
                });

            // Add label background
            nodeEnter.append('rect')
                .attr('class', 'node-label-bg')
                .attr('rx', 4)
                .attr('ry', 4)
                .style("fill", function(d) {
                    return d.data.type === "module" ? badgeBg : "transparent";
                })
                .style("opacity", 0);

            // Add text label
            nodeEnter.append('text')
                .attr('dy', '.35em')
                .attr('x', function(d) {
                    return d.children || d._children ? -13 : 13;
                })
                .style("fill", textColor)
                .style("fill-opacity", 1e-6)
                .style("font-size", "13px")
                .style("font-family", "'JetBrains Mono', monospace")
                .text(function(d) {
                    var label = d.data.name;
                    if (d.data.type === "function") {
                        label += d.data.signature ? "(" + d.data.signature + ")" : "";
                    }
                    return label;
                });

            // Add type badge
            nodeEnter.append('text')
                .attr('class', 'node-badge')
                .attr('dy', '.35em')
                .attr('x', function(d) {
                    return d.children || d._children ? -13 : 13;
                })
                .style("fill", badgeFg)
                .style("fill-opacity", 1e-6)
                .style("font-size", "10px")
                .style("font-family", "'JetBrains Mono', monospace")
                .style("font-weight", "500")
                .text(function(d) {
                    if (d.data.type === "module") return "📦 module";
                    if (d.data.type === "type") return "🔷 type";
                    if (d.data.type === "function") return "⚡ fn";
                    if (d.data.type === "category") return "📁 " + d.data.name;
                    return "";
                });

            // UPDATE

            var nodeUpdate = nodeEnter.merge(node);

            // Transition to the proper position for the node
            nodeUpdate.transition()
                .duration(duration)
                .attr("transform", function(d) {
                    return "translate(" + d.y + "," + d.x + ")";
                });

            // Update the node attributes and style
            nodeUpdate.select('circle.node-circle')
                .attr('r', 10)
                .style("fill", function(d) {
                    return d._children ? nodeStroke : (d.children ? nodeStroke : nodeFill);
                })
                .style("stroke", nodeStroke)
                .style("cursor", function(d) {
                    return d.children || d._children ? "pointer" : "default";
                });

            nodeUpdate.select('text')
                .style("fill", textColor)
                .style("fill-opacity", 1);

            nodeUpdate.select('text.node-badge')
                .style("fill", badgeFg)
                .style("fill-opacity", function(d) {
                    return d.data.type === "module" || d.data.type === "type" || d.data.type === "function" || d.data.type === "category" ? 1 : 0;
                });

            // Transition label backgrounds
            nodeUpdate.select('rect.node-label-bg')
                .style("opacity", function(d) {
                    return d.data.type === "module" ? 1 : 0;
                })
                .attr('width', function(d) {
                    // Will be set after text is rendered
                    return 0;
                })
                .attr('height', 18)
                .attr('y', -9)
                .attr('x', function(d) {
                    return d.children || d._children ? -10 : 20;
                });

            // Remove any on-screen nodes that have been moved off-screen
            var nodeExit = nodeUpdate.exit().transition()
                .duration(duration)
                .attr("transform", function(d) {
                    return "translate(" + source.y + "," + source.x + ")";
                })
                .remove();

            nodeExit.select('circle')
                .attr('r', 1e-6);

            nodeExit.select('text')
                .style("fill-opacity", 1e-6);

            nodeExit.select('rect')
                .style("opacity", 0);

            // ****************** Links section ******************

            var links = root.links();

            var link = g.selectAll('path.link')
                .data(links, function(d) { return d.target.id; });

            // Enter any new links at the parent's previous position.
            var linkEnter = link.enter().insert('path', "g")
                .attr("class", "link")
                .style("stroke", linkColor)
                .style("stroke-width", 1.5)
                .style("fill", "none")
                .attr('d', function(d) {
                    var o = {x: source.x0, y: source.y0};
                    return diagonal(o, o);
                });

            // UPDATE
            var linkUpdate = linkEnter.merge(link);

            // Transition back to the parent element position
            linkUpdate.transition()
                .duration(duration)
                .attr('d', function(d) { return diagonal(d.source, d.target); });

            // Remove any on-screen links that have been moved off-screen
            var linkExit = linkUpdate.exit().transition()
                .duration(duration)
                .attr('d', function(d) {
                    var o = {x: source.x, y: source.y};
                    return diagonal(o, o);
                })
                .remove();

            // Store the old positions for transition.
            nodes.forEach(function(d) {
                d.x0 = d.x;
                d.y0 = d.y;
            });

            // Curved links
            function diagonal(s, d) {
                return 'M ' + s.y + ' ' + s.x
                    + ' C ' + (s.y + 100) + ' ' + s.x
                    + ' ' + (d.y - 100) + ' ' + d.x
                    + ' ' + d.y + ' ' + d.x;
            }
        }

        function click(d) {
            if (d.children) {
                d._children = d.children;
                d.children = null;
            } else {
                d.children = d._children;
                d._children = null;
            }
        }

        var nodeId = 0;

        function countNodes(d) {
            var count = 1;
            if (d.children) {
                d.children.forEach(function(c) { count += countNodes(c); });
            }
            return count;
        }

        // Adjust label positions after render
        setTimeout(function() {
            g.selectAll('g.node').each(function(d) {
                var g = d3.select(this);
                var text = g.select('text:not(.node-badge)');
                var bbox = text.node() ? text.node().getBBox() : {width: 0};
                var bg = g.select('rect.node-label-bg');
                if (d.data.type === "module" && bbox.width > 0) {
                    bg
                        .attr('width', bbox.width + 10)
                        .attr('x', (d.children || d._children) ? -5 : 15)
                        .attr('y', -10)
                        .attr('height', 20);
                }
            });
        }, duration + 50);
    }

    // Initialize when DOM is ready
    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", init);
    } else {
        init();
    }
})();
