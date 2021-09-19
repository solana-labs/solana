import ContentLoader from 'react-content-loader';

export const ThreeDots = () => (
    <ContentLoader
        viewBox="0 0 212 200"
        height={200}
        width={212}
        backgroundColor="transparent"
        style={{
            width: '100%',
            margin: 'auto',
        }}
    >
        <circle cx="86" cy="100" r="8" />
        <circle cx="106" cy="100" r="8" />
        <circle cx="126" cy="100" r="8" />
    </ContentLoader>
);