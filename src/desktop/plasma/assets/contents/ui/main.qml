/*
 * wsx panel indicator
 *
 * Polls `wsx waybar status` -- the same JSON the waybar module renders -- and
 * shows a branch icon plus the live workspace count, tinted by the most urgent
 * workspace status, with the per-workspace list as its tooltip. Clicking opens
 * the workspaces from `wsx workspace list --json`; picking one runs
 * `wsx waybar jump`, which opens it in a running TUI or launches one.
 *
 * Installed by `wsx setup plasma`; edits are overwritten on re-run.
 */
import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.kde.plasma.components as PlasmaComponents
import org.kde.plasma.core as PlasmaCore
import org.kde.plasma.extras as PlasmaExtras
import org.kde.plasma.plasma5support as Plasma5Support
import org.kde.plasma.plasmoid

PlasmoidItem {
    id: root

    // Substituted at install time: the wsx binary `wsx setup plasma` resolved,
    // shell-quoted, because plasmashell's PATH often lacks ~/.local/bin.
    readonly property string wsx: __WSX_BIN__

    // The last status payload. An empty statusText means no repos are
    // registered or the database couldn't be read: the waybar module hides
    // itself then, but a panel applet can't, so it dims instead.
    property string statusText: ""
    property string statusClass: "idle"
    property string statusTooltip: ""
    property string runError: ""
    // The last `workspace list --json` output, to skip rebuilding an
    // unchanged list; empty until the first one arrives.
    property string rowsJson: ""

    readonly property string count: {
        var m = statusText.match(/(\d+)\s*$/);
        return m ? m[1] : "";
    }
    readonly property color statusColor: stateColor(statusClass)

    // The waybar stylesheet's four classes, mapped onto the color scheme's
    // message colors so the indicator follows the user's theme.
    function stateColor(state) {
        switch (state) {
        case "blocked": return Kirigami.Theme.negativeTextColor;
        case "done": return Kirigami.Theme.activeTextColor;
        case "waiting": return Kirigami.Theme.neutralTextColor;
        case "working": case "busy": return Kirigami.Theme.positiveTextColor;
        default: return Kirigami.Theme.textColor;
        }
    }

    // The tooltip's glyphs (desktop::rows::state_glyph).
    function stateGlyph(state) {
        switch (state) {
        case "blocked": return "!";
        case "done": return "✓";
        case "waiting": return "…";
        case "working": case "busy": return "↻";
        default: return "·";
        }
    }

    toolTipMainText: "wsx"
    toolTipSubText: {
        if (runError.length) return runError;
        if (statusTooltip.length) return statusTooltip;
        return "No workspaces";
    }
    // The tooltip arrives Pango-escaped for waybar and is unescaped below, so
    // it must render as plain text: a status message is agent-authored.
    toolTipTextFormat: Text.PlainText

    function unescapePango(s) {
        // &amp; last, so an escaped "&lt;" doesn't decode twice.
        return s.replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&");
    }

    function applyStatus(text) {
        try {
            var d = JSON.parse(text);
        } catch (e) {
            return;
        }
        statusText = d.text || "";
        statusClass = d["class"] || "idle";
        statusTooltip = unescapePango(d.tooltip || "");
        runError = "";
    }

    function applyRows(text) {
        try {
            var rows = JSON.parse(text);
        } catch (e) {
            return;
        }
        rowsJson = text;
        rowsModel.clear();
        for (var i = 0; i < rows.length; i++) {
            var r = rows[i];
            var status = r.status || {};
            // ListModel roles can't hold null.
            rowsModel.append({
                repo: r.repo,
                slug: r.slug,
                reportedState: status.state || "",
                message: status.message || "",
                pr: r.pr && r.pr.number ? "#" + r.pr.number : "",
            });
        }
    }

    function shellQuote(s) {
        return "'" + s.replace(/'/g, "'\\''") + "'";
    }

    function jump(repo, slug) {
        runner.connectSource(wsx + " waybar jump " + shellQuote(repo) + " " + shellQuote(slug));
        expanded = false;
    }

    ListModel {
        id: rowsModel
    }

    Plasma5Support.DataSource {
        engine: "executable"
        connectedSources: [root.wsx + " waybar status"]
        // The waybar module's poll interval.
        interval: 5000
        onNewData: function(sourceName, data) {
            // `wsx waybar status` always exits 0, so anything else means the
            // binary itself couldn't run (moved, deleted, ...).
            if (data["exit code"] === 0) {
                root.applyStatus(data["stdout"]);
            } else {
                root.statusText = "";
                root.statusClass = "idle";
                root.runError = "Could not run " + sourceName + "\n"
                    + (data["stderr"] || "").trim();
            }
        }
    }

    // The popup's rows, polled only while it's open.
    Plasma5Support.DataSource {
        engine: "executable"
        connectedSources: root.expanded ? [root.wsx + " workspace list --json"] : []
        interval: 5000
        onNewData: function(sourceName, data) {
            if (data["exit code"] === 0 && data["stdout"] !== root.rowsJson) {
                root.applyRows(data["stdout"]);
            }
        }
    }

    // One-shot commands, dropped once they finish.
    Plasma5Support.DataSource {
        id: runner
        engine: "executable"
        onNewData: function(sourceName) {
            disconnectSource(sourceName);
        }
    }

    compactRepresentation: MouseArea {
        property bool wasExpanded: false

        Layout.minimumWidth: indicator.implicitWidth
        Layout.minimumHeight: indicator.implicitHeight

        // Read on press: the popup has already closed by the time a click
        // on the icon lands, so toggling `expanded` would reopen it.
        onPressed: wasExpanded = root.expanded
        onClicked: root.expanded = !wasExpanded

        GridLayout {
            id: indicator

            readonly property bool vertical:
                Plasmoid.formFactor === PlasmaCore.Types.Vertical

            anchors.centerIn: parent
            flow: vertical ? GridLayout.TopToBottom : GridLayout.LeftToRight
            columnSpacing: Kirigami.Units.smallSpacing
            rowSpacing: Kirigami.Units.smallSpacing
            opacity: root.statusText.length ? 1.0 : 0.5

            Kirigami.Icon {
                Layout.alignment: Qt.AlignCenter
                Layout.preferredWidth: Kirigami.Units.iconSizes.small
                Layout.preferredHeight: Kirigami.Units.iconSizes.small
                source: "vcs-branch"
                // A mask, so it takes the status color like the waybar glyph.
                isMask: true
                color: root.statusColor
            }

            PlasmaComponents.Label {
                Layout.alignment: Qt.AlignCenter
                visible: root.count.length > 0
                text: root.count
                color: root.statusColor
            }
        }
    }

    fullRepresentation: PlasmaExtras.Representation {
        Layout.minimumWidth: Kirigami.Units.gridUnit * 18
        Layout.minimumHeight: Kirigami.Units.gridUnit * 10
        Layout.preferredWidth: Kirigami.Units.gridUnit * 24
        Layout.preferredHeight: Kirigami.Units.gridUnit * 22
        collapseMarginsHint: true

        contentItem: PlasmaComponents.ScrollView {
            // Wrap to the popup's width instead of scrolling sideways.
            contentWidth: availableWidth

            ListView {
                id: list

                model: rowsModel
                section.property: "repo"
                section.delegate: PlasmaExtras.ListSectionHeader {
                    required property string section

                    width: ListView.view.width
                    text: section
                }

                delegate: PlasmaComponents.ItemDelegate {
                    required property string repo
                    required property string slug
                    required property string reportedState
                    required property string message
                    required property string pr

                    width: ListView.view.width
                    onClicked: root.jump(repo, slug)

                    contentItem: RowLayout {
                        spacing: Kirigami.Units.smallSpacing

                        PlasmaComponents.Label {
                            Layout.preferredWidth: Kirigami.Units.gridUnit
                            horizontalAlignment: Text.AlignHCenter
                            text: root.stateGlyph(reportedState)
                            color: root.stateColor(reportedState)
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 0

                            PlasmaComponents.Label {
                                Layout.fillWidth: true
                                text: slug
                                textFormat: Text.PlainText
                                elide: Text.ElideRight
                            }
                            PlasmaComponents.Label {
                                Layout.fillWidth: true
                                visible: message.length > 0
                                text: message
                                textFormat: Text.PlainText
                                elide: Text.ElideRight
                                font: Kirigami.Theme.smallFont
                                opacity: 0.7
                            }
                        }

                        PlasmaComponents.Label {
                            visible: pr.length > 0
                            text: pr
                            opacity: 0.7
                        }
                    }
                }

                PlasmaExtras.PlaceholderMessage {
                    anchors.centerIn: parent
                    width: parent.width - Kirigami.Units.gridUnit * 4
                    // Not before the first list arrives, or it flashes on open.
                    visible: root.rowsJson.length > 0 && list.count === 0
                    iconName: "vcs-branch"
                    text: "No workspaces"
                }
            }
        }
    }
}
