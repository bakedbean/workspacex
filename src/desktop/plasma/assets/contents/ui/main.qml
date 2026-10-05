/*
 * wsx panel indicator
 *
 * Polls `wsx desktop status` and shows a branch icon plus the live workspace
 * count, tinted by the most urgent workspace status, with the per-workspace
 * list as its tooltip. Clicking opens the same payload's rows; picking one
 * runs `wsx desktop jump`, which opens it in a running TUI or launches one.
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

    // The last `wsx desktop status` payload. Its tooltip is empty when no
    // repo is registered: the waybar module hides itself then, but a panel
    // applet can't, so it dims instead, as it does when the status can't be
    // read.
    property int count: 0
    property string statusClass: "idle"
    property string statusTooltip: ""
    // Why the status couldn't be read: wsx failed to run, or it reported an
    // error reading its database.
    property string statusError: ""
    // Polls in a row whose database read failed. The usual cause is a
    // database busy with the dashboard's writes, which clears by the next
    // poll or two.
    property int failedReads: 0
    // The last rows, serialized, to skip rebuilding an unchanged list; empty
    // until the first payload arrives.
    property string rowsJson: ""
    // Why the last jump failed, shown in the popup until the next one.
    property string jumpError: ""

    readonly property bool hasStatus: statusTooltip.length > 0 && statusError.length === 0
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
        case "done": return "\u2713";
        case "waiting": return "\u2026";
        case "working": case "busy": return "\u21bb";
        default: return "\u00b7";
        }
    }

    toolTipMainText: "wsx"
    toolTipSubText: {
        if (statusError.length) return statusError;
        if (statusTooltip.length) return statusTooltip;
        return "No workspaces";
    }
    // A status message is agent-authored, so it must never render as markup.
    toolTipTextFormat: Text.PlainText

    function applyStatus(text) {
        try {
            var d = JSON.parse(text);
        } catch (e) {
            return;
        }
        if (d.error) {
            // Keep the last good status through a brief failure, and show the
            // error once it lasts three polls, or at once with nothing to show.
            failedReads += 1;
            if (failedReads >= 3 || rowsJson.length === 0) {
                statusError = "Could not read wsx's status:\n" + d.error;
            }
            return;
        }
        failedReads = 0;
        statusError = "";
        count = d.count || 0;
        statusClass = d["class"] || "idle";
        statusTooltip = d.tooltip || "";
        applyRows(d.rows || []);
    }

    function applyRows(rows) {
        var json = JSON.stringify(rows);
        if (json === rowsJson) {
            return;
        }
        rowsJson = json;
        rowsModel.clear();
        for (var i = 0; i < rows.length; i++) {
            var r = rows[i];
            // ListModel roles can't hold null.
            rowsModel.append({
                repo: r.repo,
                slug: r.slug,
                reportedState: r.state || "",
                message: r.message || "",
                pr: r.pr_number ? "#" + r.pr_number : "",
            });
        }
    }

    function shellQuote(s) {
        return "'" + s.replace(/'/g, "'\\''") + "'";
    }

    function jump(repo, slug) {
        jumpError = "";
        runner.connectSource(wsx + " desktop jump " + shellQuote(repo) + " " + shellQuote(slug));
    }

    // A failed jump's message is about that attempt, not the next opening.
    onExpandedChanged: if (root.expanded) jumpError = ""

    ListModel {
        id: rowsModel
    }

    Plasma5Support.DataSource {
        engine: "executable"
        connectedSources: [root.wsx + " desktop status"]
        // The waybar module's poll interval.
        interval: 5000
        onNewData: function(sourceName, data) {
            // `wsx desktop status` reports a database it can't read in its
            // output, so a failed command means wsx itself couldn't run
            // (moved, deleted, ...).
            if (data["exit code"] === 0) {
                root.applyStatus(data["stdout"]);
            } else {
                root.statusError = "Could not run " + sourceName + "\n"
                    + (data["stderr"] || "").trim();
            }
        }
    }

    // One `wsx desktop jump` per pick. The popup closes once a jump succeeds;
    // a failure stays up in it, since a terminal that won't launch would
    // otherwise close the popup with nothing happening.
    Plasma5Support.DataSource {
        id: runner
        engine: "executable"
        onNewData: function(sourceName, data) {
            disconnectSource(sourceName);
            if (data["exit code"] === 0) {
                root.expanded = false;
            } else {
                root.jumpError = (data["stderr"] || "").trim()
                    || "wsx desktop jump exited with status " + data["exit code"];
            }
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
            opacity: root.hasStatus ? 1.0 : 0.5

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
                visible: root.hasStatus
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

        contentItem: ColumnLayout {
            spacing: 0

            Kirigami.InlineMessage {
                Layout.fillWidth: true
                type: Kirigami.MessageType.Error
                text: root.jumpError || root.statusError
                visible: text.length > 0
            }

            PlasmaComponents.ScrollView {
                Layout.fillWidth: true
                Layout.fillHeight: true
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
}
