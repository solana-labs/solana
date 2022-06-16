import { Html, Head, Main, NextScript } from 'next/document'
import Script from 'next/script'

export default function Document() {
	return (
		<Html>
			<Head>
        <link
          href="https://fonts.googleapis.com/css?family=Oswald|Rubik|Exo+2:300,400,700|Saira|Saira+Condensed|Saira+Extra+Condensed|Saira+Semi+Condensed&display=swap"
          rel="stylesheet"
        />

        <Script src="https://www.googletagmanager.com/gtag/js?id=UA-112467444-2" strategy="afterInteractive" />
        <Script id="init-google-analytics" strategy="afterInteractive">
          {`
            window.dataLayer = window.dataLayer || [];
            function gtag(){dataLayer.push(arguments);}
            gtag('js', new Date());

            gtag('config', 'UA-112467444-2');
          `}
        </Script>
      </Head>
			<body>
				<Main />
				<NextScript />
			</body>
		</Html>
	)
}
