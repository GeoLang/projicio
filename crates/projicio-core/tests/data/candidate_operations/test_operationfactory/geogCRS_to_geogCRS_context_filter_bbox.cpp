TEST(operation, geogCRS_to_geogCRS_context_filter_bbox) {
    auto authFactory =
        AuthorityFactory::create(DatabaseContext::create(), "EPSG");
    // INSERT INTO "area" VALUES('EPSG','1197','Romania','Romania - onshore and
    // offshore.',43.44,48.27,20.26,31.41,0);
    {
        auto ctxt = CoordinateOperationContext::create(
            authFactory, Extent::createFromBBOX(20.26, 43.44, 31.41, 48.27),
            0.0);
        auto list = CoordinateOperationFactory::create()->createOperations(
            authFactory->createCoordinateReferenceSystem("4179"),
            authFactory->createCoordinateReferenceSystem("4258"), ctxt);
        ASSERT_EQ(list.size(), 1U);
        EXPECT_EQ(list[0]->getEPSGCode(), 15994); // Romania - 3m
    }
    {
        auto ctxt = CoordinateOperationContext::create(
            authFactory,
            Extent::createFromBBOX(20.26 + .1, 43.44 + .1, 31.41 - .1,
                                   48.27 - .1),
            0.0);
        auto list = CoordinateOperationFactory::create()->createOperations(
            authFactory->createCoordinateReferenceSystem("4179"),
            authFactory->createCoordinateReferenceSystem("4258"), ctxt);
        ASSERT_EQ(list.size(), 1U);
        EXPECT_EQ(list[0]->getEPSGCode(), 15994); // Romania - 3m
    }
    {
        auto ctxt = CoordinateOperationContext::create(
            authFactory,
            Extent::createFromBBOX(20.26 - .1, 43.44 - .1, 31.41 + .1,
                                   48.27 + .1),
            0.0);
        auto list = CoordinateOperationFactory::create()->createOperations(
            authFactory->createCoordinateReferenceSystem("4179"),
            authFactory->createCoordinateReferenceSystem("4258"), ctxt);
        ASSERT_EQ(list.size(), 1U);
        EXPECT_EQ(
            list[0]->exportToPROJString(PROJStringFormatter::create().get()),
            "+proj=noop");
    }
}

