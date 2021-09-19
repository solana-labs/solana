import React from 'react';
import '@google/model-viewer/dist/model-viewer';

type MeshViewerProps = {
    className?: string;
    url?: string;
    style?: React.CSSProperties;
    onError?: () => void;
};

export function MeshViewer(props: MeshViewerProps) {
    return (
        // @ts-ignore
        <model-viewer
            style={{
                width: `100%`,
                height: `100%`,
                minHeight: 400,
                minWidth: 400,
                maxHeight: 400,
                ...props.style,
            }}
            src={props.url}
            auto-rotate
            rotation-per-second="40deg"
            className={props.className}
            camera-controls
        />
    );
}