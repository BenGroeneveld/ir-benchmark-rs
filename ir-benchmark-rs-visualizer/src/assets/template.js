(function () {
    const controlConfig = __CONTROL_CONFIG_JSON__;
    const chartPanels = Array.from(document.querySelectorAll(".chart-panel"));
    const activeChartTitle = document.getElementById("active-chart-title");
    const chartNavButtons = Array.from(document.querySelectorAll("[data-chart-nav]"));
    const chartControlsPanel = document.querySelector(".controls-panel");
    const chartViewerPanel = document.getElementById("chart-viewer-panel");
    const diffTablePanel = document.getElementById("diff-table-panel");
    let activeViewPanelIndex = 0;

    function isTypingTarget(target) {
        if (!target) {
            return false;
        }
        const tagName = target.tagName ? target.tagName.toLowerCase() : "";
        return (
            target.isContentEditable ||
            tagName === "input" ||
            tagName === "textarea" ||
            tagName === "select"
        );
    }

    function getActiveChartPanel() {
        const focusedPanel = document.activeElement && document.activeElement.closest
            ? document.activeElement.closest(".chart-panel")
            : null;
        return focusedPanel || document.querySelector(".chart-panel.is-active") || chartPanels[0] || null;
    }

    function setActiveChartPanel(panel, focusPanel = false) {
        chartPanels.forEach((item) => {
            const isActive = item === panel;
            item.classList.toggle("is-active", isActive);
            item.hidden = !isActive;
        });

        if (activeChartTitle && panel) {
            const heading = panel.querySelector("h2");
            activeChartTitle.textContent = heading ? heading.textContent : "Chart";
        }

        const plot = panel ? panel.querySelector(".js-plotly-plot") : null;
        if (plot && typeof Plotly !== "undefined") {
            window.requestAnimationFrame(() => {
                Plotly.Plots.resize(plot);
            });
        }

        if (focusPanel && panel) {
            panel.focus({ preventScroll: true });
        }
    }

    function moveChartSelection(direction) {
        if (!chartPanels.length) {
            return;
        }

        const currentPanel = getActiveChartPanel();
        const currentIndex = Math.max(0, chartPanels.indexOf(currentPanel));
        const delta = direction === "next" ? 1 : -1;
        const nextIndex = (currentIndex + delta + chartPanels.length) % chartPanels.length;
        setActiveChartPanel(chartPanels[nextIndex], true);
    }

    function wireChartSelection() {
        if (!chartPanels.length) {
            return;
        }

        setActiveChartPanel(chartPanels[0], false);

        chartPanels.forEach((panel) => {
            panel.addEventListener("focus", () => setActiveChartPanel(panel, false));
            panel.addEventListener("click", () => setActiveChartPanel(panel, false));
        });

        chartNavButtons.forEach((button) => {
            button.addEventListener("click", () => {
                const direction = button.getAttribute("data-chart-nav");
                moveChartSelection(direction === "next" ? "next" : "previous");
            });
        });

        document.addEventListener("keydown", (event) => {
            if (isTypingTarget(event.target)) {
                return;
            }

            if (event.key === "ArrowRight") {
                event.preventDefault();
                moveChartSelection("next");
            } else if (event.key === "ArrowLeft") {
                event.preventDefault();
                moveChartSelection("previous");
            } else if (event.key === "ArrowDown") {
                event.preventDefault();
                focusPanelForKeyboard(diffTablePanel);
            } else if (event.key === "ArrowUp") {
                event.preventDefault();
                focusPanelForKeyboard(chartViewerPanel);
            }
        });
    }

    function getViewPanels() {
        return [chartViewerPanel, diffTablePanel].filter((panel) => !!panel);
    }

    function focusPanelForKeyboard(panel) {
        if (!panel) {
            return;
        }

        const scrollTarget =
            panel === chartViewerPanel && chartControlsPanel
                ? chartControlsPanel
                : panel;
        scrollTarget.scrollIntoView({ behavior: "smooth", block: "start" });

        if (panel === chartViewerPanel) {
            const activeChartPanel = getActiveChartPanel();
            if (activeChartPanel) {
                setActiveChartPanel(activeChartPanel, true);
            } else {
                panel.focus({ preventScroll: true });
            }
            return;
        }

        panel.focus({ preventScroll: true });
    }

    function wireViewPanelTracking() {
        const viewPanels = getViewPanels();
        if (!viewPanels.length) {
            return;
        }

        activeViewPanelIndex = 0;

        viewPanels.forEach((panel, panelIndex) => {
            panel.addEventListener("focusin", () => {
                activeViewPanelIndex = panelIndex;
            });
            panel.addEventListener("click", () => {
                activeViewPanelIndex = panelIndex;
            });
        });
    }

    function moveViewSelection(direction) {
        const viewPanels = getViewPanels();
        if (viewPanels.length < 2) {
            return;
        }

        const delta = direction === "next" ? 1 : -1;
        activeViewPanelIndex =
            (activeViewPanelIndex + delta + viewPanels.length) % viewPanels.length;
        focusPanelForKeyboard(viewPanels[activeViewPanelIndex]);
    }

    function getPlot() {
        return document.querySelector(".js-plotly-plot");
    }

    function setLabelState(label, checked) {
        label.classList.toggle("is-checked", checked);
    }

    function syncLabelStates() {
        document.querySelectorAll(".control-item").forEach((label) => {
            const input = label.querySelector("input[type='checkbox']");
            if (input) {
                setLabelState(label, input.checked);
            }
        });
    }
    function applyControlState() {
        const plot = getPlot();
        if (!plot || typeof Plotly === "undefined") {
            return;
        }

        const minMaxToggle = document.getElementById("control-show-minmax");
        const fpsToggle = document.getElementById("control-show-fps-lines");
        const meanFillToggle = document.getElementById("control-show-mean-fill");

        const minMaxVisible = !!(minMaxToggle && minMaxToggle.checked);
        const fpsVisible = !!(fpsToggle && fpsToggle.checked);
        const meanFillVisible = !!(meanFillToggle && meanFillToggle.checked);

        Plotly.restyle(
            plot,
            {
                fillcolor: controlConfig.min_max_fill_colors.map((color) =>
                    minMaxVisible ? color : "rgba(0, 0, 0, 0)"
                ),
                "line.color": controlConfig.min_max_line_colors.map((color) =>
                    minMaxVisible ? color : "rgba(0, 0, 0, 0)"
                ),
            },
            controlConfig.min_max_trace_indices
        );

        Plotly.restyle(
            plot,
            {
                fillcolor: controlConfig.mean_fill_colors.map((color) =>
                    meanFillVisible ? color : "rgba(0, 0, 0, 0)"
                ),
            },
            controlConfig.mean_fill_trace_indices
        );

        const relayoutUpdates = {};
        controlConfig.fps_shape_indices.forEach((index) => {
            relayoutUpdates[`shapes[${index}].visible`] = fpsVisible;
        });
        controlConfig.fps_annotation_indices.forEach((index) => {
            relayoutUpdates[`annotations[${index}].visible`] = fpsVisible;
        });
        Plotly.relayout(plot, relayoutUpdates);
    }

    function wireControls() {
        syncLabelStates();
        applyControlState();

        document.querySelectorAll(".control-item input[type='checkbox']").forEach((input) => {
            input.addEventListener("change", () => {
                const label = input.closest(".control-item");
                if (label) {
                    setLabelState(label, input.checked);
                }
                applyControlState();
            });
        });
    }

    function getAllChartPlots() {
        return chartPanels
            .map((panel) => panel.querySelector(".js-plotly-plot"))
            .filter((plot) => !!plot);
    }

    function wireThemeSelector(retryCount = 0) {
        const themeSelect = document.getElementById("theme-select");
        if (!themeSelect) {
            return;
        }

        const applyTheme = (selectedTheme) => {
            const isDark = selectedTheme === "plotly_dark";
            const pageTheme = isDark ? "dark" : "light";
            document.body.setAttribute("data-page-theme", pageTheme);
            document.documentElement.style.colorScheme = pageTheme;

            const relayoutUpdates = {
                "paper_bgcolor": isDark ? "#111111" : "#FFFFFF",
                "plot_bgcolor": isDark ? "#111111" : "#FFFFFF",
                "font.color": isDark ? "#f2f2f2" : "#2a3f5f",
                "xaxis.gridcolor": isDark ? "#283442" : "#EBF0F8",
                "xaxis.linecolor": isDark ? "#506784" : "#EBF0F8",
                "xaxis.zerolinecolor": isDark ? "#283442" : "#EBF0F8",
                "yaxis.gridcolor": isDark ? "#283442" : "#EBF0F8",
                "yaxis.linecolor": isDark ? "#506784" : "#EBF0F8",
                "yaxis.zerolinecolor": isDark ? "#283442" : "#EBF0F8",
            };

            getAllChartPlots().forEach((plot) => {
                if (plot && typeof Plotly !== "undefined") {
                    Plotly.relayout(plot, relayoutUpdates);
                }
            });

            recalculateDiffCellStylesForCurrentOrder();
            recalculatePerformanceSummaryForCurrentOrder();
        };

        themeSelect.addEventListener("change", (event) => {
            applyTheme(event.target.value);
        });

        // Force the initial theme to the current selector value (defaults to Plotly Dark).
        themeSelect.value = themeSelect.value || "plotly_dark";
        const plots = getAllChartPlots();
        if (!plots.length && retryCount < 40) {
            window.setTimeout(() => wireThemeSelector(retryCount + 1), 150);
            return;
        }
        applyTheme(themeSelect.value);
    }

    function getGroupCheckboxes() {
        return Array.from(document.querySelectorAll(".group-visibility-checkbox[data-group]"));
    }

    function getGroupVisibilityByCheckbox() {
        const visibility = new Map();
        getGroupCheckboxes().forEach((checkbox) => {
            const groupName = checkbox.getAttribute("data-group") || "";
            if (!groupName) {
                return;
            }
            visibility.set(groupName, !!checkbox.checked);
        });
        return visibility;
    }

    function applyCheckboxStateToLinkedLegend(visibilityByGroup) {
        document.querySelectorAll(".legend-row[data-group]").forEach((row) => {
            const groupName = row.getAttribute("data-group") || "";
            const isVisible = visibilityByGroup.get(groupName) !== false;
            row.classList.toggle("group-unselected", !isVisible);
        });
    }

    function applyCheckboxStateToComparisonTable(visibilityByGroup) {
        document.querySelectorAll(".ini-comparison-table [data-group]").forEach((element) => {
            const groupName = element.getAttribute("data-group") || "";
            const isVisible = visibilityByGroup.get(groupName) !== false;
            element.classList.toggle("group-unselected", !isVisible);
            element.classList.toggle("group-filtered-out", !isVisible);
        });
    }

    function applyCheckboxStateToCharts(visibilityByGroup) {
        if (typeof Plotly === "undefined") {
            return;
        }

        getAllChartPlots().forEach((plot) => {
            if (!plot || !Array.isArray(plot.data)) {
                return;
            }

            const traceIndices = [];
            const visibilityValues = [];

            plot.data.forEach((trace, traceIndex) => {
                const groupName = typeof trace.legendgroup === "string" ? trace.legendgroup : "";
                if (!groupName || !visibilityByGroup.has(groupName)) {
                    return;
                }

                traceIndices.push(traceIndex);
                visibilityValues.push(visibilityByGroup.get(groupName) !== false);
            });

            if (traceIndices.length) {
                Plotly.restyle(plot, { visible: visibilityValues }, traceIndices);
            }
        });
    }

    function getLegendRowsInOrder() {
        return Array.from(document.querySelectorAll(".linked-legend .legend-row[data-group]"));
    }

    function getLegendGroupOrder() {
        return getLegendRowsInOrder()
            .map((row) => row.getAttribute("data-group") || "")
            .filter((name) => !!name);
    }

    function reorderComparisonTableColumnsByLegend() {
        const orderedGroups = getLegendGroupOrder();
        if (!orderedGroups.length) {
            return;
        }

        document.querySelectorAll(".ini-comparison-table").forEach((table) => {
            table.querySelectorAll("tr").forEach((row) => {
                const groupedCells = Array.from(row.children).filter(
                    (cell) => cell.matches && cell.matches("[data-group]")
                );
                if (!groupedCells.length) {
                    return;
                }

                const groupedMap = new Map();
                groupedCells.forEach((cell) => {
                    const groupName = cell.getAttribute("data-group") || "";
                    if (groupName) {
                        groupedMap.set(groupName, cell);
                    }
                });

                orderedGroups.forEach((groupName) => {
                    const cell = groupedMap.get(groupName);
                    if (cell) {
                        row.appendChild(cell);
                    }
                });
            });
        });
    }

    function getTooltipTextForTableCell(cell) {
        if (!(cell instanceof HTMLElement)) {
            return "";
        }

        const rawValue = cell.getAttribute("data-raw-value");
        if (typeof rawValue === "string" && rawValue.trim()) {
            return rawValue.replace(/\r\n?/g, "\n").trim();
        }

        const fallback = typeof cell.innerText === "string" ? cell.innerText : (cell.textContent || "");
        return fallback.replace(/\r\n?/g, "\n").trim();
    }

    function isOverflowingElement(element) {
        if (!(element instanceof HTMLElement)) {
            return false;
        }

        return (
            element.scrollWidth > element.clientWidth + 1 ||
            element.scrollHeight > element.clientHeight + 1
        );
    }

    function refreshComparisonTableTruncationTooltips() {
        document.querySelectorAll(".ini-comparison-table th, .ini-comparison-table td").forEach((cell) => {
            if (!(cell instanceof HTMLElement)) {
                return;
            }

            const tooltipText = getTooltipTextForTableCell(cell);
            const shouldShowTooltip = !!tooltipText && isOverflowingElement(cell);

            cell.classList.toggle("is-truncated", shouldShowTooltip);
            if (shouldShowTooltip) {
                cell.setAttribute("title", tooltipText);
            } else {
                cell.removeAttribute("title");
            }
        });
    }

    function tryParseNumber(value) {
        if (typeof value !== "string") {
            return Number.NaN;
        }
        const trimmed = value.trim();
        if (!trimmed) {
            return Number.NaN;
        }
        const parsed = Number(trimmed);
        return Number.isFinite(parsed) ? parsed : Number.NaN;
    }

    function getCurrentBaselineGroup() {
        const orderedGroups = getLegendGroupOrder();
        if (!orderedGroups.length) {
            return "";
        }

        const visibilityByGroup = getGroupVisibilityByCheckbox();
        const firstVisible = orderedGroups.find((groupName) => visibilityByGroup.get(groupName) !== false);
        return firstVisible || "";
    }

    function getVisibleGroupCount() {
        return getGroupCheckboxes().filter((checkbox) => checkbox.checked).length;
    }

    function getIniSectionRows(headerRow) {
        const rows = [];
        if (!(headerRow instanceof HTMLElement) || !headerRow.parentElement) {
            return rows;
        }

        let currentRow = headerRow.nextElementSibling;
        while (currentRow instanceof HTMLElement) {
            if (currentRow.classList.contains("table-major-section-row") || currentRow.classList.contains("table-subsection-row")) {
                break;
            }

            rows.push(currentRow);
            currentRow = currentRow.nextElementSibling;
        }

        return rows;
    }

    function updateIniSectionVisibleRowCounts() {
        document.querySelectorAll(".ini-comparison-table tbody > tr.table-subsection-row").forEach((headerRow) => {
            if (!(headerRow instanceof HTMLElement)) {
                return;
            }

            const sectionRows = getIniSectionRows(headerRow);
            const totalRowCount = sectionRows.length;
            const diffRowCount = sectionRows.filter((row) => {
                return row.classList.contains("changed-row") && (row.getAttribute("data-row-has-diff") || "false") === "true";
            }).length;
            const countElement = headerRow.querySelector("[data-visible-row-count]");
            if (countElement) {
                const rowLabel = totalRowCount === 1 ? "total" : "total";
                const diffLabel = diffRowCount === 1 ? "diff" : "diffs";
                const isCollapsed = (headerRow.getAttribute("data-collapsed") || "true") === "true";
                const displayMode = isCollapsed ? "showing diffs" : "showing all";
                countElement.textContent = `${displayMode} | ${diffRowCount} ${diffLabel} / ${totalRowCount} ${rowLabel}`;
            }
        });
    }

    function recalculateDiffCellStylesForCurrentOrder() {
        const isLightTheme = (document.body.getAttribute("data-page-theme") || "dark") === "light";
        const diffPalette = isLightTheme
            ? {
                baseline: { color: "#2a5e93", background: "linear-gradient(90deg, rgba(47, 110, 163, 0.30), rgba(47, 110, 163, 0.08))" },
                equal: { color: "#2f6ea3", background: "linear-gradient(90deg, rgba(47, 110, 163, 0.24), rgba(47, 110, 163, 0.07))" },
                better: { color: "#1f7a2f", background: "linear-gradient(90deg, rgba(79, 174, 104, 0.36), rgba(79, 174, 104, 0.10))" },
                worse: { color: "#a63a3a", background: "linear-gradient(90deg, rgba(214, 86, 86, 0.36), rgba(214, 86, 86, 0.10))" },
            }
            : {
                baseline: { color: "#9ccfff", background: "linear-gradient(90deg, rgba(107, 163, 208, 0.24), rgba(107, 163, 208, 0.07))" },
                equal: { color: "#8ec7ff", background: "linear-gradient(90deg, rgba(107, 163, 208, 0.22), rgba(107, 163, 208, 0.06))" },
                better: { color: "#a5f09d", background: "linear-gradient(90deg, rgba(124, 197, 118, 0.32), rgba(124, 197, 118, 0.09))" },
                worse: { color: "#ff9f9f", background: "linear-gradient(90deg, rgba(255, 107, 107, 0.32), rgba(255, 107, 107, 0.09))" },
            };

        const baselineGroup = getCurrentBaselineGroup();
        if (!baselineGroup) {
            return;
        }

        const changedRows = Array.from(document.querySelectorAll(".ini-comparison-table tr.changed-row"));
        changedRows.forEach((row) => {
            const valueCells = Array.from(row.querySelectorAll("td.value-cell[data-group]"));
            if (!valueCells.length) {
                return;
            }

            const baselineCell = valueCells.find(
                (cell) => (cell.getAttribute("data-group") || "") === baselineGroup
            );
            if (!baselineCell) {
                return;
            }

            const baselineHasValue = (baselineCell.getAttribute("data-has-value") || "false") === "true";
            const baselineRaw = baselineCell.getAttribute("data-raw-value") || "";
            const baselineNumber = tryParseNumber(baselineRaw);

            valueCells.forEach((cell) => {
                if (cell.classList.contains("group-filtered-out")) {
                    cell.removeAttribute("data-visible-changed");
                    return;
                }

                const groupName = cell.getAttribute("data-group") || "";
                if (groupName === baselineGroup) {
                    cell.style.color = diffPalette.baseline.color;
                    cell.style.background = diffPalette.baseline.background;
                    cell.setAttribute("data-diff-state", "baseline");
                    cell.setAttribute("data-diff-direction", "baseline");
                    cell.setAttribute("data-visible-changed", "false");
                    return;
                }

                const hasValue = (cell.getAttribute("data-has-value") || "false") === "true";
                const raw = cell.getAttribute("data-raw-value") || "";
                const numeric = tryParseNumber(raw);
                let diffDirection = "equal";

                if (!baselineHasValue && hasValue) {
                    diffDirection = "better";
                } else if (baselineHasValue && !hasValue) {
                    diffDirection = "worse";
                } else if (baselineHasValue && hasValue) {
                    if (Number.isFinite(baselineNumber) && Number.isFinite(numeric)) {
                        if (numeric > baselineNumber) {
                            diffDirection = "better";
                        } else if (numeric < baselineNumber) {
                            diffDirection = "worse";
                        }
                    } else {
                        diffDirection = raw !== baselineRaw ? "worse" : "equal";
                    }
                }

                const paletteForCell = diffPalette[diffDirection] || diffPalette.equal;
                const isChanged = diffDirection === "better" || diffDirection === "worse";
                cell.style.color = paletteForCell.color;
                cell.style.background = paletteForCell.background;
                cell.setAttribute("data-diff-state", isChanged ? "changed" : "equal");
                cell.setAttribute("data-diff-direction", diffDirection);
                cell.setAttribute("data-visible-changed", isChanged ? "true" : "false");
            });
        });
    }

    function refreshIniDifferenceRowVisibility() {
        document.querySelectorAll(".ini-comparison-table").forEach((table) => {
            const rows = Array.from(table.querySelectorAll("tbody > tr"));

            function rowHasAnyVisibleDifference(row) {
                const visibleValueCells = Array.from(
                    row.querySelectorAll("td.value-cell[data-group]:not(.group-filtered-out)")
                );
                if (visibleValueCells.length <= 1) {
                    return false;
                }

                const firstCell = visibleValueCells[0];
                const firstHasValue = (firstCell.getAttribute("data-has-value") || "false") === "true";
                const firstRawValue = firstCell.getAttribute("data-raw-value") || "";

                return visibleValueCells.slice(1).some((cell) => {
                    const hasValue = (cell.getAttribute("data-has-value") || "false") === "true";
                    const rawValue = cell.getAttribute("data-raw-value") || "";
                    return hasValue !== firstHasValue || rawValue !== firstRawValue;
                });
            }

            let activeSubsectionHeader = null;
            let activeSubsectionCollapsed = true;

            const finalizeActiveSubsection = () => {
                if (!activeSubsectionHeader) {
                    return;
                }

                activeSubsectionHeader.setAttribute(
                    "aria-expanded",
                    activeSubsectionCollapsed ? "false" : "true"
                );
                activeSubsectionHeader.classList.toggle("is-collapsed", activeSubsectionCollapsed);
            };

            rows.forEach((row) => {
                if (row.classList.contains("table-major-section-row")) {
                    finalizeActiveSubsection();
                    activeSubsectionHeader = null;
                    activeSubsectionCollapsed = true;
                    row.hidden = false;
                    return;
                }

                if (row.classList.contains("table-subsection-row")) {
                    finalizeActiveSubsection();
                    activeSubsectionHeader = row;
                    activeSubsectionCollapsed = (row.getAttribute("data-collapsed") || "true") === "true";
                    row.hidden = false;
                    return;
                }

                if (!activeSubsectionHeader) {
                    return;
                }

                let rowShouldBeVisible = true;

                if (row.classList.contains("changed-row")) {
                    const hasVisibleDifference = rowHasAnyVisibleDifference(row);
                    rowShouldBeVisible = activeSubsectionCollapsed ? hasVisibleDifference : true;
                    row.setAttribute(
                        "data-row-has-diff",
                        hasVisibleDifference ? "true" : "false"
                    );
                } else if (!row.classList.contains("notes-collapsible-row")) {
                    rowShouldBeVisible = true;
                }

                row.setAttribute("data-filter-visible", rowShouldBeVisible ? "true" : "false");
                if (!row.classList.contains("changed-row")) {
                    row.removeAttribute("data-row-has-diff");
                }

                row.hidden = !rowShouldBeVisible;
            });

            finalizeActiveSubsection();
        });

        updateIniSectionVisibleRowCounts();
    }

    function wireIniSectionCollapseControls() {
        document.querySelectorAll(".ini-comparison-table tbody > tr.table-subsection-row").forEach((headerRow) => {
            if (!(headerRow instanceof HTMLElement)) {
                return;
            }

            if (headerRow.getAttribute("data-collapse-wired") === "true") {
                return;
            }

            headerRow.setAttribute("data-collapse-wired", "true");

            headerRow.setAttribute("role", "button");
            headerRow.setAttribute("tabindex", "0");
            headerRow.setAttribute("aria-expanded", (headerRow.getAttribute("data-collapsed") || "true") === "true" ? "false" : "true");
            headerRow.title = "Click to show or hide same-value rows";
            headerRow.style.cursor = "pointer";

            headerRow.addEventListener("click", () => {
                const currentlyCollapsed = (headerRow.getAttribute("data-collapsed") || "true") === "true";
                headerRow.setAttribute("data-collapsed", currentlyCollapsed ? "false" : "true");
                refreshIniDifferenceRowVisibility();
            });

            headerRow.addEventListener("keydown", (event) => {
                if (event.key !== "Enter" && event.key !== " ") {
                    return;
                }

                event.preventDefault();
                headerRow.click();
            });
        });
    }

    function buildSummaryMetricHtml(currentMs, baselineMs, includeIndicator) {
        const hasCurrent = Number.isFinite(currentMs);
        const hasBaseline = Number.isFinite(baselineMs);

        let state = "equal";
        if (hasCurrent && hasBaseline) {
            if (currentMs > baselineMs) {
                state = "worse";
            } else if (currentMs < baselineMs) {
                state = "better";
            }
        }

        const indicatorLayoutClass = includeIndicator ? "summary-has-indicator" : "summary-no-indicator";
        let indicatorHtml = "";
        if (includeIndicator) {
            const symbol = state === "better" ? "&#9650;" : state === "worse" ? "&#9660;" : "&equals;";
            indicatorHtml = `<span class="perf-indicator perf-indicator-${state}">${symbol}</span>`;
        }

        let relativePerfText = includeIndicator ? "N/A perf" : "100% perf";
        if (includeIndicator && hasCurrent && hasBaseline && currentMs > 0 && baselineMs > 0) {
            const relativePerfPct = ((baselineMs / currentMs) - 1.0) * 100.0;
            relativePerfText = `${relativePerfPct >= 0 ? "+" : ""}${relativePerfPct.toFixed(1)}% perf`;
        }

        if (!hasCurrent) {
            return (
                `<span class="summary-metric-block summary-state-${state} ${indicatorLayoutClass}">` +
                indicatorHtml +
                `<span class="summary-delta">${relativePerfText}</span>` +
                '<span class="summary-main-value">N/A</span>' +
                "</span>"
            );
        }

        const fps = currentMs > 0 ? (1000.0 / currentMs) : 0.0;
        return (
            `<span class="summary-metric-block summary-state-${state} ${indicatorLayoutClass}">` +
            indicatorHtml +
            `<span class="summary-delta">${relativePerfText}</span>` +
            `<span class="summary-main-value">${currentMs.toFixed(2)} ms</span>` +
            `<span class="summary-subvalue">${fps.toFixed(1)} fps</span>` +
            "</span>"
        );
    }

    function getSummaryState(currentMs, baselineMs, isBaselineGroup) {
        if (isBaselineGroup) {
            return "baseline";
        }

        const hasCurrent = Number.isFinite(currentMs);
        const hasBaseline = Number.isFinite(baselineMs);
        if (!hasCurrent || !hasBaseline) {
            return "equal";
        }

        if (currentMs < baselineMs) {
            return "better";
        }
        if (currentMs > baselineMs) {
            return "worse";
        }
        return "equal";
    }

    function getSummaryCellBackgroundColor(state) {
        const isLightTheme = (document.body.getAttribute("data-page-theme") || "dark") === "light";
        if (state === "better") {
            return isLightTheme
                ? "linear-gradient(90deg, rgba(79, 174, 104, 0.36), rgba(79, 174, 104, 0.10))"
                : "linear-gradient(90deg, rgba(124, 197, 118, 0.32), rgba(124, 197, 118, 0.09))";
        }
        if (state === "worse") {
            return isLightTheme
                ? "linear-gradient(90deg, rgba(214, 86, 86, 0.36), rgba(214, 86, 86, 0.10))"
                : "linear-gradient(90deg, rgba(255, 107, 107, 0.32), rgba(255, 107, 107, 0.09))";
        }
        if (state === "baseline") {
            return isLightTheme
                ? "linear-gradient(90deg, rgba(47, 110, 163, 0.30), rgba(47, 110, 163, 0.08))"
                : "linear-gradient(90deg, rgba(107, 163, 208, 0.24), rgba(107, 163, 208, 0.07))";
        }
        return isLightTheme
            ? "linear-gradient(90deg, rgba(47, 110, 163, 0.24), rgba(47, 110, 163, 0.07))"
            : "linear-gradient(90deg, rgba(107, 163, 208, 0.22), rgba(107, 163, 208, 0.06))";
    }

    function recalculatePerformanceSummaryForCurrentOrder() {
        const orderedGroups = getLegendGroupOrder();
        const baselineGroup = getCurrentBaselineGroup();
        if (!baselineGroup) {
            return;
        }

        const summaryRows = Array.from(
            document.querySelectorAll(".ini-comparison-table tr:has(td.stat-value[data-metric-key])")
        );

        summaryRows.forEach((row) => {
            const summaryCells = Array.from(row.querySelectorAll("td.stat-value[data-group][data-metric-key]"));
            if (!summaryCells.length) {
                return;
            }

            const cellByGroup = new Map();
            summaryCells.forEach((cell) => {
                const group = cell.getAttribute("data-group") || "";
                if (group) {
                    cellByGroup.set(group, cell);
                }
            });

            const baselineCell = cellByGroup.get(baselineGroup);
            const baselineMs = baselineCell
                ? tryParseNumber(baselineCell.getAttribute("data-frametime-ms") || "")
                : Number.NaN;

            orderedGroups.forEach((groupName) => {
                const cell = cellByGroup.get(groupName);
                if (!cell) {
                    return;
                }
                const currentMs = tryParseNumber(cell.getAttribute("data-frametime-ms") || "");
                const includeIndicator = groupName !== baselineGroup;
                const state = getSummaryState(currentMs, baselineMs, !includeIndicator);
                cell.setAttribute("data-summary-state", state);
                cell.style.background = getSummaryCellBackgroundColor(state);
                cell.innerHTML = buildSummaryMetricHtml(currentMs, baselineMs, includeIndicator);
            });
        });
    }

    function refreshComparisonDataFromLegendOrder() {
        reorderComparisonTableColumnsByLegend();
        recalculateDiffCellStylesForCurrentOrder();
        refreshIniDifferenceRowVisibility();
        wireIniSectionCollapseControls();
        recalculatePerformanceSummaryForCurrentOrder();
        refreshComparisonTableTruncationTooltips();
    }

    function wireLegendDragAndDrop() {
        const legendContainer = document.querySelector(".linked-legend");
        if (!legendContainer) {
            return;
        }

        let draggingRow = null;

        function clearDragTargets() {
            getLegendRowsInOrder().forEach((row) => row.classList.remove("drag-target"));
        }

        legendContainer.addEventListener("dragstart", (event) => {
            const target = event.target instanceof Element
                ? event.target.closest(".legend-row[data-group]")
                : null;
            if (!(target instanceof HTMLElement)) {
                return;
            }
            draggingRow = target;
            draggingRow.classList.add("dragging");
            if (event.dataTransfer) {
                event.dataTransfer.effectAllowed = "move";
                event.dataTransfer.setData("text/plain", target.getAttribute("data-group") || "");
            }
        });

        legendContainer.addEventListener("dragover", (event) => {
            if (!draggingRow) {
                return;
            }
            event.preventDefault();
            if (event.dataTransfer) {
                event.dataTransfer.dropEffect = "move";
            }

            const target = event.target instanceof Element
                ? event.target.closest(".legend-row[data-group]")
                : null;
            if (!(target instanceof HTMLElement) || target === draggingRow) {
                clearDragTargets();
                return;
            }

            clearDragTargets();
            target.classList.add("drag-target");

            const bounds = target.getBoundingClientRect();
            const dropAfter = event.clientY >= (bounds.top + bounds.height / 2);
            if (dropAfter) {
                legendContainer.insertBefore(draggingRow, target.nextElementSibling);
            } else {
                legendContainer.insertBefore(draggingRow, target);
            }
        });

        legendContainer.addEventListener("drop", (event) => {
            if (!draggingRow) {
                return;
            }
            event.preventDefault();
            clearDragTargets();
            refreshComparisonDataFromLegendOrder();
        });

        legendContainer.addEventListener("dragend", () => {
            if (!draggingRow) {
                return;
            }
            draggingRow.classList.remove("dragging");
            clearDragTargets();
            draggingRow = null;
            refreshComparisonDataFromLegendOrder();
        });
    }

    function getLegendRowByGroupName(groupName) {
        return getLegendRowsInOrder().find((row) => (row.getAttribute("data-group") || "") === groupName) || null;
    }

    function moveLegendGroupBeforeOrAfter(draggingGroup, targetGroup, dropAfter) {
        const legendContainer = document.querySelector(".linked-legend");
        if (!legendContainer) {
            return false;
        }

        const draggingRow = getLegendRowByGroupName(draggingGroup);
        const targetRow = getLegendRowByGroupName(targetGroup);
        if (!draggingRow || !targetRow || draggingRow === targetRow) {
            return false;
        }

        if (dropAfter) {
            legendContainer.insertBefore(draggingRow, targetRow.nextElementSibling);
        } else {
            legendContainer.insertBefore(draggingRow, targetRow);
        }
        return true;
    }

    function wireComparisonTableColumnDragAndDrop() {
        const table = document.querySelector(".ini-comparison-table");
        if (!table) {
            return;
        }

        let draggingGroup = "";

        function getGroupHeaderFromEvent(event) {
            const target = event.target instanceof Element
                ? event.target.closest("th.group-column-header[data-group]")
                : null;
            return target instanceof HTMLElement ? target : null;
        }

        function clearDropHints() {
            table.querySelectorAll("th.group-column-header[data-group]").forEach((header) => {
                if (!(header instanceof HTMLElement)) {
                    return;
                }
                header.style.boxShadow = "";
            });
        }

        table.querySelectorAll("th.group-column-header[data-group]").forEach((header) => {
            if (!(header instanceof HTMLElement)) {
                return;
            }
            header.draggable = true;
            header.style.cursor = "grab";
            header.title = "Drag to reorder groups";
        });

        table.addEventListener("dragstart", (event) => {
            const header = getGroupHeaderFromEvent(event);
            if (!header) {
                return;
            }

            draggingGroup = header.getAttribute("data-group") || "";
            if (!draggingGroup) {
                return;
            }

            header.style.cursor = "grabbing";
            if (event.dataTransfer) {
                event.dataTransfer.effectAllowed = "move";
                event.dataTransfer.setData("text/plain", draggingGroup);
            }
        });

        table.addEventListener("dragover", (event) => {
            if (!draggingGroup) {
                return;
            }

            const header = getGroupHeaderFromEvent(event);
            if (!header) {
                clearDropHints();
                return;
            }

            const targetGroup = header.getAttribute("data-group") || "";
            if (!targetGroup || targetGroup === draggingGroup) {
                clearDropHints();
                return;
            }

            event.preventDefault();
            if (event.dataTransfer) {
                event.dataTransfer.dropEffect = "move";
            }

            clearDropHints();
            const bounds = header.getBoundingClientRect();
            const dropAfter = event.clientX >= (bounds.left + bounds.width / 2);
            header.style.boxShadow = dropAfter
                ? "inset -3px 0 0 rgba(107, 163, 208, 0.9)"
                : "inset 3px 0 0 rgba(107, 163, 208, 0.9)";
        });

        table.addEventListener("drop", (event) => {
            if (!draggingGroup) {
                return;
            }

            const header = getGroupHeaderFromEvent(event);
            clearDropHints();
            if (!header) {
                return;
            }

            const targetGroup = header.getAttribute("data-group") || "";
            if (!targetGroup || targetGroup === draggingGroup) {
                return;
            }

            event.preventDefault();

            const bounds = header.getBoundingClientRect();
            const dropAfter = event.clientX >= (bounds.left + bounds.width / 2);
            const moved = moveLegendGroupBeforeOrAfter(draggingGroup, targetGroup, dropAfter);
            if (moved) {
                refreshComparisonDataFromLegendOrder();
            }
        });

        table.addEventListener("dragend", () => {
            clearDropHints();
            table.querySelectorAll("th.group-column-header[data-group]").forEach((header) => {
                if (header instanceof HTMLElement) {
                    header.style.cursor = "grab";
                }
            });
            draggingGroup = "";
        });
    }

    function applyGroupVisibilityFromCheckboxes() {
        const visibilityByGroup = getGroupVisibilityByCheckbox();
        applyCheckboxStateToLinkedLegend(visibilityByGroup);
        applyCheckboxStateToComparisonTable(visibilityByGroup);
        applyCheckboxStateToCharts(visibilityByGroup);
        refreshComparisonDataFromLegendOrder();
    }

    function wireGroupVisibilityCheckboxes(retryCount = 0) {
        const checkboxes = getGroupCheckboxes();
        if (!checkboxes.length) {
            return;
        }

        const plots = getAllChartPlots();
        if (!plots.length && retryCount < 40) {
            window.setTimeout(() => wireGroupVisibilityCheckboxes(retryCount + 1), 150);
            return;
        }

        checkboxes.forEach((checkbox) => {
            checkbox.addEventListener("change", () => {
                applyGroupVisibilityFromCheckboxes();
            });
        });

        const checkAllButton = document.getElementById("linked-legend-check-all");
        if (checkAllButton) {
            checkAllButton.addEventListener("click", () => {
                checkboxes.forEach((checkbox) => {
                    checkbox.checked = true;
                });
                applyGroupVisibilityFromCheckboxes();
            });
        }

        const uncheckAllButton = document.getElementById("linked-legend-uncheck-all");
        if (uncheckAllButton) {
            uncheckAllButton.addEventListener("click", () => {
                checkboxes.forEach((checkbox) => {
                    checkbox.checked = false;
                });
                applyGroupVisibilityFromCheckboxes();
            });
        }

        applyGroupVisibilityFromCheckboxes();
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", () => {
            wireControls();
            wireLegendDragAndDrop();
            wireComparisonTableColumnDragAndDrop();
            wireGroupVisibilityCheckboxes();
            refreshComparisonDataFromLegendOrder();
            wireViewPanelTracking();
            wireThemeSelector();
            window.addEventListener("resize", refreshComparisonTableTruncationTooltips);
        });
    } else {
        wireControls();
        wireLegendDragAndDrop();
        wireComparisonTableColumnDragAndDrop();
        wireGroupVisibilityCheckboxes();
        refreshComparisonDataFromLegendOrder();
        wireViewPanelTracking();
        wireThemeSelector();
        window.addEventListener("resize", refreshComparisonTableTruncationTooltips);
    }

    wireChartSelection();
})();

